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

/// Read a single image off the pasteboard as a PNG `data:` URL, or None if it
/// holds no image. A clipboard image is usually present in several formats at
/// once (PNG + TIFF + …); the webview's native paste inserts one copy per
/// format, so the rich editor reads exactly one representation here instead and
/// skips the native paste. Screenshots and most copies already carry a PNG;
/// otherwise an NSImage is pulled and re-encoded to PNG.
#[cfg(target_os = "macos")]
pub fn read_image_data_url() -> Option<String> {
    use base64::Engine;
    use objc2::{AllocAnyThread, ClassType};
    use objc2_app_kit::{NSBitmapImageFileType, NSBitmapImageRep, NSImage, NSPasteboard};
    use objc2_foundation::{NSArray, NSDictionary, NSString};

    let png: Vec<u8> = unsafe {
        let pb = NSPasteboard::generalPasteboard();
        let png_type = NSString::from_str("public.png");
        if let Some(data) = pb.dataForType(&png_type) {
            data.to_vec()
        } else {
            let classes = NSArray::from_slice(&[NSImage::class()]);
            let objs = pb.readObjectsForClasses_options(&classes, None)?;
            let img = objs.iter().find_map(|o| o.downcast::<NSImage>().ok())?;
            let tiff = img.TIFFRepresentation()?;
            let rep = NSBitmapImageRep::initWithData(NSBitmapImageRep::alloc(), &tiff)?;
            let props = NSDictionary::new();
            let data =
                rep.representationUsingType_properties(NSBitmapImageFileType::PNG, &props)?;
            data.to_vec()
        }
    };
    if png.is_empty() {
        return None;
    }
    let b64 = base64::engine::general_purpose::STANDARD.encode(&png);
    Some(format!("data:image/png;base64,{b64}"))
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
#[cfg(not(target_os = "macos"))]
pub fn read_image_data_url() -> Option<String> {
    None
}

// Headless native-layer test: put one image on the real pasteboard in several
// formats at once (as a real copy does) and assert the reader returns exactly
// one PNG data: URL — the property the desktop paste fix relies on. Ignored by
// default because it clobbers the shared system clipboard; run explicitly with
// `cargo test -p harbly-app -- --ignored reads_a_single_png`.
#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::read_image_data_url;
    use base64::Engine;
    use objc2_app_kit::NSPasteboard;
    use objc2_foundation::{NSArray, NSData, NSString};

    const PNG_B64: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+M8AAAMBAQDJ/pLvAAAAAElFTkSuQmCC";

    #[test]
    #[ignore = "reads/writes the shared system clipboard"]
    fn reads_a_single_png_from_a_multi_format_clipboard() {
        let png = base64::engine::general_purpose::STANDARD
            .decode(PNG_B64)
            .unwrap();
        unsafe {
            let pb = NSPasteboard::generalPasteboard();
            let png_type = NSString::from_str("public.png");
            let tiff_type = NSString::from_str("public.tiff");
            let types = NSArray::from_slice(&[&*png_type, &*tiff_type]);
            pb.clearContents();
            pb.declareTypes_owner(&types, None);
            let data = NSData::with_bytes(&png);
            pb.setData_forType(Some(&data), &png_type);
            pb.setData_forType(Some(&data), &tiff_type);
        }
        let url = read_image_data_url().expect("an image should be read");
        assert!(
            url.starts_with("data:image/png;base64,"),
            "unexpected prefix: {}",
            &url[..url.len().min(48)]
        );
        assert!(url.len() > "data:image/png;base64,".len());
    }
}
