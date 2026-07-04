//! File interchange with the system clipboard (macOS NSPasteboard):
//! - ⌘C in the app → write file URLs, so ⌘V works directly in Finder;
//! - ⌘C in Finder → ⌘V in the app reads the file paths back and pastes them into the library.
//!
//! Also provides a bridge that forwards edit actions like copy:/paste: to the first
//! responder — since the menu's ⌘C/⌘V are taken over by custom items, text editing
//! inside input fields still goes through the system responder chain.

use std::path::PathBuf;

#[cfg(target_os = "macos")]
pub fn write_file_urls(paths: &[PathBuf]) -> Result<(), String> {
    use objc2::runtime::ProtocolObject;
    use objc2_app_kit::{NSPasteboard, NSPasteboardWriting};
    use objc2_foundation::{NSArray, NSString, NSURL};

    let objs: Vec<objc2::rc::Retained<ProtocolObject<dyn NSPasteboardWriting>>> = paths
        .iter()
        .filter_map(|p| p.to_str())
        .map(|s| ProtocolObject::from_retained(NSURL::fileURLWithPath(&NSString::from_str(s))))
        .collect();
    if objs.is_empty() {
        return Err("没有可拷贝的文件".into());
    }
    let pb = NSPasteboard::generalPasteboard();
    pb.clearContents();
    if pb.writeObjects(&NSArray::from_retained_slice(&objs)) {
        Ok(())
    } else {
        Err("写入剪贴板失败".into())
    }
}

#[cfg(target_os = "macos")]
pub fn read_file_paths() -> Vec<PathBuf> {
    use objc2::ClassType;
    use objc2_app_kit::NSPasteboard;
    use objc2_foundation::{NSArray, NSURL};

    unsafe {
        let pb = NSPasteboard::generalPasteboard();
        let classes = NSArray::from_slice(&[NSURL::class()]);
        let Some(objs) = pb.readObjectsForClasses_options(&classes, None) else {
            return vec![];
        };
        objs.iter()
            .filter_map(|o| {
                o.downcast_ref::<NSURL>()
                    .and_then(|u| u.path())
                    .map(|p| PathBuf::from(p.to_string()))
            })
            .collect()
    }
}

/// Send an edit action to the first responder (equivalent to what the system's
/// predefined menu items do). Must be called on the main thread.
#[cfg(target_os = "macos")]
pub fn forward_responder_action(action: &str) -> Result<(), String> {
    use objc2::sel;
    use objc2_app_kit::NSApplication;

    let mtm = unsafe { objc2::MainThreadMarker::new_unchecked() };
    let sel = match action {
        "copy" => sel!(copy:),
        "paste" => sel!(paste:),
        "cut" => sel!(cut:),
        "selectAll" => sel!(selectAll:),
        // ⌘⌫ semantics in text editing (forwarded back to the input field after
        // the menu takes over the shortcut)
        "deleteToLineStart" => sel!(deleteToBeginningOfLine:),
        _ => return Err("未知编辑动作".into()),
    };
    let app = NSApplication::sharedApplication(mtm);
    unsafe {
        app.sendAction_to_from(sel, None, None);
    }
    Ok(())
}

#[cfg(not(target_os = "macos"))]
pub fn write_file_urls(_: &[PathBuf]) -> Result<(), String> {
    Err("仅 macOS 支持文件拷贝".into())
}
#[cfg(not(target_os = "macos"))]
pub fn read_file_paths() -> Vec<PathBuf> {
    vec![]
}
#[cfg(not(target_os = "macos"))]
pub fn forward_responder_action(_: &str) -> Result<(), String> {
    Ok(())
}
