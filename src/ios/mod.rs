//! iPadOS pieces (docs/IPAD-PLAN.md): everything here is compiled for iOS only.
//!
//! * the renderer: eframe on iOS is wgpu over Metal, since glutin has no iOS backend;
//! * the library lives in the app's Documents folder, which the Files app shows;
//! * the audio session is set for playback that mixes with other apps.

pub mod hotkeys;

use std::path::PathBuf;

/// The app's Documents folder (the app container's, as `HOME` says).
pub fn documents_dir() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(std::env::temp_dir).join("Documents")
}

/// Where the library is kept: a folder in Documents, visible in the Files app.
pub fn library_dir() -> PathBuf {
    documents_dir().join("LeadStinger")
}

/// wgpu's defaults, with the limits clamped to what the adapter offers: the simulator's
/// Metal allows 15 inter-stage variables and wgpu's default asks for 16, which fails
/// device creation.
pub fn wgpu_options() -> eframe::egui_wgpu::WgpuConfiguration {
    use eframe::egui_wgpu::{WgpuConfiguration, WgpuSetup};
    let mut config = WgpuConfiguration::default();
    if let WgpuSetup::CreateNew(setup) = &mut config.wgpu_setup {
        let base = setup.device_descriptor.clone();
        setup.device_descriptor = std::sync::Arc::new(move |adapter| {
            let mut descriptor = base(adapter);
            let offered = adapter.limits();
            let limits = &mut descriptor.required_limits;
            limits.max_inter_stage_shader_variables = limits.max_inter_stage_shader_variables.min(offered.max_inter_stage_shader_variables);
            descriptor
        });
    }
    config
}

/// Playback that mixes with other apps (a soundboard runs next to a music app or a
/// call) and keeps playing with the silent switch on. Called before the audio engine
/// opens its stream.
pub fn start_audio_session() {
    use objc2_avf_audio::{AVAudioSession, AVAudioSessionCategoryOptions, AVAudioSessionCategoryPlayback, AVAudioSessionModeDefault};
    // SAFETY: the audio session constants are valid for the life of the process.
    unsafe {
        let session = AVAudioSession::sharedInstance();
        if let Err(e) = session.setCategory_mode_options_error(
            AVAudioSessionCategoryPlayback.unwrap(),
            AVAudioSessionModeDefault.unwrap(),
            AVAudioSessionCategoryOptions::MixWithOthers,
        ) {
            eprintln!("leadstinger: couldn't set the audio session category: {e}");
        }
        if let Err(e) = session.setActive_error(true) {
            eprintln!("leadstinger: couldn't activate the audio session: {e}");
        }
    }
}
