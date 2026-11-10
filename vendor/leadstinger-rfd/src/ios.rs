//! The part of `rfd` LeadStinger uses (`FileDialog`), for iPadOS, where rfd has no
//! backend. Picking opens the system's document picker (the Files app) and waits for it
//! by running the main run loop, so the caller gets its answer as it does on desktop.
//! The picked files are copies in the app's temporary folder.
//!
//! There is no save panel to wait for: `save_file` and `pick_folder` answer with a path
//! in the app's `Documents/Exports` folder, which the Files app shows.
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::{define_class, msg_send, DefinedClass, MainThreadMarker, MainThreadOnly};
use objc2_foundation::{NSArray, NSDate, NSDefaultRunLoopMode, NSObject, NSObjectProtocol, NSRunLoop, NSString, NSURL};
#[allow(deprecated)]
use objc2_ui_kit::{UIApplication, UIDocumentPickerDelegate, UIDocumentPickerMode, UIDocumentPickerViewController, UIWindowScene};
use std::cell::{Cell, RefCell};
use std::path::PathBuf;

#[derive(Default)]
struct Answer {
    done: Cell<bool>,
    files: RefCell<Vec<PathBuf>>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "LeadStingerPickerDelegate"]
    #[ivars = Answer]
    struct Delegate;

    unsafe impl NSObjectProtocol for Delegate {}

    unsafe impl UIDocumentPickerDelegate for Delegate {
        #[unsafe(method(documentPicker:didPickDocumentsAtURLs:))]
        fn did_pick(&self, _picker: &UIDocumentPickerViewController, urls: &NSArray<NSURL>) {
            let files = urls.iter().filter_map(|u| u.path().map(|p| PathBuf::from(p.to_string()))).collect();
            *self.ivars().files.borrow_mut() = files;
            self.ivars().done.set(true);
        }

        #[unsafe(method(documentPickerWasCancelled:))]
        fn cancelled(&self, _picker: &UIDocumentPickerViewController) {
            self.ivars().done.set(true);
        }
    }
);

impl Delegate {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(Answer::default());
        unsafe { msg_send![super(this), init] }
    }
}

#[derive(Default)]
pub struct FileDialog {
    title: Option<String>,
    file_name: Option<String>,
}

/// Show the picker for `types` (UTIs) and wait for it.
fn pick(types: &[&str], multiple: bool) -> Vec<PathBuf> {
    let Some(mtm) = MainThreadMarker::new() else { return Vec::new() };
    let app = UIApplication::sharedApplication(mtm);
    let root = app.connectedScenes().iter().find_map(|scene| {
        let scene = scene.downcast::<UIWindowScene>().ok()?;
        #[allow(deprecated)]
        scene.keyWindow()?.rootViewController()
    });
    let Some(root) = root else { return Vec::new() };
    let types: Retained<NSArray<NSString>> = NSArray::from_retained_slice(&types.iter().map(|t| NSString::from_str(t)).collect::<Vec<_>>());
    #[allow(deprecated)]
    let picker = UIDocumentPickerViewController::initWithDocumentTypes_inMode(UIDocumentPickerViewController::alloc(mtm), &types, UIDocumentPickerMode::Import);
    picker.setAllowsMultipleSelection(multiple);
    let delegate = Delegate::new(mtm);
    picker.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
    root.presentViewController_animated_completion(&picker, true, None);
    let run_loop = NSRunLoop::currentRunLoop();
    while !delegate.ivars().done.get() {
        let until = NSDate::dateWithTimeIntervalSinceNow(0.05);
        unsafe { run_loop.runMode_beforeDate(NSDefaultRunLoopMode, &until) };
    }
    let files = delegate.ivars().files.take();
    files
}

fn exports_dir() -> PathBuf {
    let dir = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(std::env::temp_dir).join("Documents").join("Exports");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

impl FileDialog {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn set_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }
    /// The picker shows every file (`public.item`); what can't be read is reported by
    /// whatever imports it.
    pub fn add_filter(self, _name: impl Into<String>, _extensions: &[impl ToString]) -> Self {
        self
    }
    pub fn set_file_name(mut self, name: impl Into<String>) -> Self {
        self.file_name = Some(name.into());
        self
    }
    pub fn pick_file(self) -> Option<PathBuf> {
        pick(&["public.item"], false).into_iter().next()
    }
    pub fn pick_files(self) -> Option<Vec<PathBuf>> {
        Some(pick(&["public.item"], true)).filter(|f| !f.is_empty())
    }
    pub fn pick_folder(self) -> Option<PathBuf> {
        Some(exports_dir())
    }
    pub fn save_file(self) -> Option<PathBuf> {
        Some(exports_dir().join(self.file_name.unwrap_or_else(|| "Export".to_owned())))
    }
}
