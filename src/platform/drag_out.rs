//! 把条目拖出到外部应用（资源管理器、浏览器、编辑器…）。
//!
//! 语义与「复制 + 在目标里粘贴」一致，但由目标应用在拖放点自行决定取哪种
//! 表示：文本给纯文本，图片与文件给真实路径。
//!
//! ## 为什么不在事件处理里直接调 `DoDragDrop`
//!
//! `DoDragDrop` 是一个**阻塞的模态循环**，它在内部自己泵消息。如果在 GPUI 的
//! 鼠标事件回调里直接调用，它泵出的消息会被派发回同一个窗口，而此刻 GPUI 的
//! `App` 还处于可变借用中，于是嵌套派发触发 `RefCell already borrowed`，异常
//! 在 unwind 中再 panic 一次，最终以 `STATUS_STACK_BUFFER_OVERRUN`
//! （0xC0000409）直接结束进程。
//!
//! 所以这里拆成两步：UI 侧只把条目写进剪贴板并 `PostMessage` 一个私有消息，
//! 真正的 `DoDragDrop` 在主窗口的子类过程里执行 —— 那时代码运行在消息循环
//! 的派发点上，没有任何框架借用，OLE 泵回来的消息可以正常走 GPUI 的窗口过程。
//!
//! ## 数据表示
//!
//! 先写剪贴板（走内部的「不要记录自己这次写」通道），再用 `OleGetClipboard`
//! 取回系统数据对象交给 `DoDragDrop`。这样文本 / HTML / RTF / 文件 / 图片五种
//! 表示都不需要自己实现 `IDataObject`，与目标应用的兼容性也和「粘贴」一致。
//! 代价是拖出后剪贴板里留着的就是被拖的那一条，与用户直接按 Ctrl+C 相同。
//!
//! macOS 目前没有实现（返回 `false`，调用方保持静默）：`NSDraggingSession`
//! 需要真实的鼠标事件与视图句柄，未在本机验证过，先不写不可测的代码。

use crate::core::types::ClipboardItem;

/// 当前平台是否支持拖出。
pub fn is_supported() -> bool {
    cfg!(target_os = "windows")
}

/// 在窗口上安装拖出所需的子类过程；主窗口拿到句柄后调用一次。
#[cfg(target_os = "windows")]
pub fn install(hwnd: isize) {
    windows_impl::install(hwnd);
}

#[cfg(not(target_os = "windows"))]
pub fn install(_hwnd: isize) {}

/// 请求把**剪贴板里当前内容**拖出到外部应用。
///
/// 调用方必须先写剪贴板（通过内部写通道避免被自己的监听器重新采集）。这里
/// 只投递消息并立即返回，真正的拖放由窗口过程执行，阻塞到用户放手或取消。
/// 返回 `false` 表示请求没能投递出去。
#[cfg(target_os = "windows")]
pub fn request_drag_out(window: &gpui::Window) -> bool {
    windows_impl::request_drag_out(window)
}

#[cfg(not(target_os = "windows"))]
pub fn request_drag_out(_window: &gpui::Window) -> bool {
    false
}

/// 判断某个条目是否值得提供拖出。
///
/// 中转站的云端条目本地没有文件，拖出去的路径是空的，因此不参与拖出。
pub fn item_can_drag_out(item: &ClipboardItem) -> bool {
    if !is_supported() {
        return false;
    }
    if item.meta_type == "transfer" {
        return false;
    }
    match item.content_type {
        crate::core::types::ContentType::File => {
            !crate::core::types::FileData::from_json(&item.file_data)
                .files
                .is_empty()
        }
        crate::core::types::ContentType::Image => !item.image_path.is_empty(),
        _ => !item.full_text.is_empty(),
    }
}

#[cfg(target_os = "windows")]
mod windows_impl {
    //! OLE `DoDragDrop` 的最小实现。
    //!
    //! `IDropSource` 只有两个方法，这里用一个静态 vtable 手工实现：
    //! 静态对象的 `AddRef`/`Release` 返回固定值，进程生命周期内不释放。
    //! 这样不需要为一次性对象写内存管理，也不引入任何新依赖。

    use std::ffi::c_void;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Once;

    use gpui::Window;
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows::core::{Interface, GUID, HRESULT};
    use windows::Win32::Foundation::BOOL;
    use windows::Win32::System::Ole::{
        DoDragDrop, IDropSource, OleGetClipboard, OleInitialize, DROPEFFECT, DROPEFFECT_COPY,
    };
    use windows_sys::Win32::{
        Foundation::{HWND, LPARAM, LRESULT, WPARAM},
        UI::{
            Shell::{DefSubclassProc, SetWindowSubclass},
            WindowsAndMessaging::PostMessageW,
        },
    };

    // 这些拖放状态码在 `windows` crate 里没有导出（它们属于 `oleidl.h` 的
    // 返回值，不属于任何方法签名），按 SDK 头文件里的字面量声明。
    const S_OK: HRESULT = HRESULT(0);
    const DRAGDROP_S_DROP: HRESULT = HRESULT(0x0004_0100);
    const DRAGDROP_S_CANCEL: HRESULT = HRESULT(0x0004_0101);
    const DRAGDROP_S_USEDEFAULTCURSORS: HRESULT = HRESULT(0x0004_0102);
    const E_NOINTERFACE: HRESULT = HRESULT(0x8000_4002u32 as i32);
    const E_POINTER: HRESULT = HRESULT(0x8000_4003u32 as i32);

    /// `MODIFIERKEYS_FLAGS::MK_LBUTTON`：左键仍按住。
    const MK_LBUTTON: u32 = 0x0001;

    /// 私有消息：在窗口过程里启动一次拖出。`WM_APP` 之后的编号归应用所有。
    const WM_APP_DRAG_OUT: u32 = 0x8000 + 0x51;
    /// 主窗口上的子类过程 id（任意且唯一即可）。
    const DRAG_SUBCLASS_ID: usize = 0x434C_5052;

    const IID_IUNKNOWN: GUID = GUID::from_u128(0x00000000_0000_0000_c000_000000000046);
    const IID_IDROPSOURCE: GUID = GUID::from_u128(0x00000121_0000_0000_c000_000000000046);

    /// 有等待执行的拖出请求。窗口过程取走后立刻清零，避免一次手势拖两次。
    static PENDING: AtomicBool = AtomicBool::new(false);
    static OLE_INIT: Once = Once::new();

    #[repr(C)]
    struct DropSourceVtbl {
        query_interface:
            unsafe extern "system" fn(*mut c_void, *const GUID, *mut *mut c_void) -> HRESULT,
        add_ref: unsafe extern "system" fn(*mut c_void) -> u32,
        release: unsafe extern "system" fn(*mut c_void) -> u32,
        query_continue_drag: unsafe extern "system" fn(*mut c_void, BOOL, u32) -> HRESULT,
        give_feedback: unsafe extern "system" fn(*mut c_void, DROPEFFECT) -> HRESULT,
    }

    unsafe impl Sync for DropSourceVtbl {}

    #[repr(C)]
    struct DropSource {
        vtbl: *const DropSourceVtbl,
    }

    unsafe impl Sync for DropSource {}

    unsafe extern "system" fn query_interface(
        this: *mut c_void,
        iid: *const GUID,
        out: *mut *mut c_void,
    ) -> HRESULT {
        if out.is_null() {
            return E_POINTER;
        }
        if iid.is_null() {
            unsafe { *out = std::ptr::null_mut() };
            return E_POINTER;
        }
        let iid = unsafe { *iid };
        if iid == IID_IUNKNOWN || iid == IID_IDROPSOURCE {
            unsafe { *out = this };
            S_OK
        } else {
            unsafe { *out = std::ptr::null_mut() };
            E_NOINTERFACE
        }
    }

    unsafe extern "system" fn add_ref(_this: *mut c_void) -> u32 {
        // 静态对象：引用计数固定为 2（OLE 最多只会配对释放一次）。
        2
    }

    unsafe extern "system" fn release(_this: *mut c_void) -> u32 {
        1
    }

    unsafe extern "system" fn query_continue_drag(
        _this: *mut c_void,
        escape_pressed: BOOL,
        key_state: u32,
    ) -> HRESULT {
        if escape_pressed.as_bool() {
            return DRAGDROP_S_CANCEL;
        }
        if key_state & MK_LBUTTON == 0 {
            return DRAGDROP_S_DROP;
        }
        S_OK
    }

    unsafe extern "system" fn give_feedback(_this: *mut c_void, _effect: DROPEFFECT) -> HRESULT {
        DRAGDROP_S_USEDEFAULTCURSORS
    }

    static DROP_SOURCE_VTBL: DropSourceVtbl = DropSourceVtbl {
        query_interface,
        add_ref,
        release,
        query_continue_drag,
        give_feedback,
    };

    static DROP_SOURCE: DropSource = DropSource {
        vtbl: &DROP_SOURCE_VTBL,
    };

    fn native_handle(window: &Window) -> Option<isize> {
        match HasWindowHandle::window_handle(window).ok()?.as_raw() {
            RawWindowHandle::Win32(handle) => Some(handle.hwnd.get()),
            _ => None,
        }
    }

    pub fn install(hwnd: isize) {
        if hwnd == 0 {
            return;
        }
        // SAFETY: 主窗口句柄存活期间一直有效；子类过程只读消息、不改窗口状态。
        let installed = unsafe {
            SetWindowSubclass(hwnd as HWND, Some(drag_subclass_proc), DRAG_SUBCLASS_ID, 0)
        };
        if installed == 0 {
            log::warn!("drag_out: could not install window subclass");
        }
    }

    /// 主窗口子类过程：只截获自己的私有消息，其余原样交回 GPUI。
    unsafe extern "system" fn drag_subclass_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
        _id: usize,
        _data: usize,
    ) -> LRESULT {
        if msg == WM_APP_DRAG_OUT && PENDING.swap(false, Ordering::SeqCst) {
            // 此刻运行在消息派发点上，没有任何 GPUI 借用：OLE 泵回的消息
            // 可以安全地走 GPUI 的窗口过程。
            run_ole_drag();
            return 0;
        }
        unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
    }

    pub fn request_drag_out(window: &Window) -> bool {
        let Some(hwnd) = native_handle(window) else {
            log::warn!("drag_out: no native window handle");
            return false;
        };
        PENDING.store(true, Ordering::SeqCst);
        // SAFETY: 只投递一个无参消息；窗口句柄在整个调用期有效。
        let posted = unsafe { PostMessageW(hwnd as HWND, WM_APP_DRAG_OUT, 0, 0) };
        if posted == 0 {
            PENDING.store(false, Ordering::SeqCst);
            log::warn!("drag_out: PostMessage failed");
            return false;
        }
        true
    }

    fn run_ole_drag() {
        unsafe {
            OLE_INIT.call_once(|| {
                // 已初始化时返回 S_FALSE，无害；失败也不阻断后续调用。
                let _ = OleInitialize(None);
            });
            let data = match OleGetClipboard() {
                Ok(data) => data,
                Err(error) => {
                    log::warn!("drag_out: OleGetClipboard failed: {error}");
                    return;
                }
            };
            let source: IDropSource =
                IDropSource::from_raw(&DROP_SOURCE as *const DropSource as *mut c_void);
            let mut effect = DROPEFFECT(0);
            let result = DoDragDrop(
                &data,
                &source,
                DROPEFFECT_COPY,
                &mut effect as *mut DROPEFFECT,
            );
            // 放手与取消都是「正常结束」，只有真正的失败才值得记一笔。
            if result != S_OK && result != DRAGDROP_S_DROP && result != DRAGDROP_S_CANCEL {
                log::warn!("drag_out: DoDragDrop failed: {result:?}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::types::{ClipboardItem, ContentType};

    fn text_item(text: &str) -> ClipboardItem {
        ClipboardItem::new_text(1, text, ContentType::PlainText, None, None)
    }

    #[test]
    fn text_entries_can_be_dragged_out() {
        assert_eq!(item_can_drag_out(&text_item("hello")), is_supported());
    }

    #[test]
    fn empty_text_is_not_draggable() {
        assert!(!item_can_drag_out(&text_item("")));
    }

    #[test]
    fn cloud_transfer_entries_are_not_draggable() {
        let mut item = text_item("remote");
        item.meta_type = "transfer".to_string();
        assert!(!item_can_drag_out(&item));
    }
}
