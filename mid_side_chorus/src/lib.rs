use nih_plug::prelude::*;
use nih_plug_egui::{create_egui_editor, egui, EguiState};
use std::sync::Arc;

use std::f32::consts::PI;

const MAX_DELAY_SAMPLES: usize = 44100 * 2; // 2 seconds at 44.1kHz

// A simple LFO.
struct Lfo {
    phase: f32,
    freq: f32,
}

impl Lfo {
    fn new(freq: f32) -> Self {
        Self { phase: 0.0, freq }
    }

    fn set_freq(&mut self, freq: f32) {
        self.freq = freq;
    }

    fn next(&mut self, sample_rate: f32) -> f32 {
        let val = (self.phase * 2.0 * PI).sin();
        self.phase += self.freq / sample_rate;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
        }
        val
    }
}


// The main plugin struct.
pub struct MidSideChorus {
    params: Arc<MidSideChorusParams>,
    editor_state: EguiState,

    // DSP state
    sample_rate: f32,
    mid_lfo1: Lfo,
    mid_lfo2: Lfo,
    side_lfo1: Lfo,
    side_lfo2: Lfo,
    mid_delay: Vec<f32>,
    side_delay: Vec<f32>,
    delay_pos: usize,
}

// The parameters for the plugin.
#[derive(Params)]
struct MidSideChorusParams {
    #[id = "mid_depth"]
    pub mid_depth: FloatParam,

    #[id = "mid_speed"]
    pub mid_speed: FloatParam,

    #[id = "side_depth"]
    pub side_depth: FloatParam,

    #[id = "side_speed"]
    pub side_speed: FloatParam,
}

impl Default for MidSideChorus {
    fn default() -> Self {
        Self {
            params: Arc::new(MidSideChorusParams::default()),
            editor_state: EguiState::from_size(300, 180),

            sample_rate: 44100.0, // Default, will be updated in initialize
            mid_lfo1: Lfo::new(0.5),
            mid_lfo2: Lfo::new(0.51), // Slightly different for stereo effect
            side_lfo1: Lfo::new(0.6),
            side_lfo2: Lfo::new(0.61),
            mid_delay: vec![0.0; MAX_DELAY_SAMPLES],
            side_delay: vec![0.0; MAX_DELAY_SAMPLES],
            delay_pos: 0,
        }
    }
}

impl Default for MidSideChorusParams {
    fn default() -> Self {
        Self {
            mid_depth: FloatParam::new(
                "Mid Depth",
                0.5,
                FloatRange::Linear { min: 0.0, max: 1.0 },
            )
            .with_smoother(SmoothingStyle::Linear(50.0)),
            mid_speed: FloatParam::new(
                "Mid Speed",
                0.5,
                FloatRange::Linear { min: 0.1, max: 10.0 },
            )
            .with_smoother(SmoothingStyle::Linear(50.0)),
            side_depth: FloatParam::new(
                "Side Depth",
                0.5,
                FloatRange::Linear { min: 0.0, max: 1.0 },
            )
            .with_smoother(SmoothingStyle::Linear(50.0)),
            side_speed: FloatParam::new(
                "Side Speed",
                0.5,
                FloatRange::Linear { min: 0.1, max: 10.0 },
            )
            .with_smoother(SmoothingStyle::Linear(50.0)),
        }
    }
}

impl Plugin for MidSideChorus {
    const NAME: &'static str = "Mid Side Chorus";
    const VENDOR: &'static str = "Jules";
    const URL: &'static str = "https://www.example.com";
    const EMAIL: &'static str = "jules@example.com";

    const VERSION: &'static str = env!("CARGO_PKG_VERSION");

    const AUDIO_IO_LAYOUTS: &'static [AudioIOLayout] = &[AudioIOLayout {
        main_input_channels: NonZeroU32::new(2),
        main_output_channels: NonZeroU32::new(2),
        ..AudioIOLayout::const_default()
    }];

    type SysExMessage = ();
    type BackgroundTask = ();

    fn params(&self) -> Arc<dyn Params> {
        self.params.clone()
    }

    fn editor(&mut self, _async_executor: AsyncExecutor<Self>) -> Option<Box<dyn Editor>> {
        let params = self.params.clone();
        create_egui_editor(
            self.editor_state.clone(),
            (),
            move |_, _| {},
            move |egui_ctx, setter, _state| {
                egui::CentralPanel::default().show(egui_ctx, |ui| {
                    ui.label("Mid");
                    ui.add(nih_plug_egui::widgets::ParamSlider::for_param(&params.mid_depth, setter));
                    ui.add(nih_plug_egui::widgets::ParamSlider::for_param(&params.mid_speed, setter));
                    ui.separator();
                    ui.label("Side");
                    ui.add(nih_plug_egui::widgets::ParamSlider::for_param(&params.side_depth, setter));
                    ui.add(nih_plug_egui::widgets::ParamSlider::for_param(&params.side_speed, setter));
                });
            },
        )
    }

    fn initialize(
        &mut self,
        _audio_io_layout: &AudioIOLayout,
        buffer_config: &BufferConfig,
        _context: &mut impl ProcessContext<Self>,
    ) -> bool {
        self.sample_rate = buffer_config.sample_rate;
        true
    }

    fn reset(&mut self) {
        self.mid_lfo1.phase = 0.0;
        self.mid_lfo2.phase = 0.0;
        self.side_lfo1.phase = 0.0;
        self.side_lfo2.phase = 0.0;
        self.mid_delay.fill(0.0);
        self.side_delay.fill(0.0);
        self.delay_pos = 0;
    }

    fn process(
        &mut self,
        buffer: &mut Buffer,
        _aux: &mut AuxiliaryBuffers,
        _context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        for mut frame in buffer.iter_samples() {
            let [left, right] = frame.get_mut(0..2).unwrap();

            // Mid/Side encoding
            let mid = (*left + *right) * 0.5;
            let side = (*left - *right) * 0.5;

            // --- Mid processing ---
            self.mid_lfo1.set_freq(self.params.mid_speed.smoothed.next());
            self.mid_lfo2.set_freq(self.params.mid_speed.smoothed.next() * 1.05); // Slightly detuned

            let mid_depth_samples = self.params.mid_depth.smoothed.next() * self.sample_rate * 0.02; // Max 20ms depth

            let mid_delay_time1 = mid_depth_samples * (1.0 + self.mid_lfo1.next(self.sample_rate));
            let mid_delay_time2 = mid_depth_samples * (1.0 + self.mid_lfo2.next(self.sample_rate));

            let mid_delayed1 = get_delayed(&self.mid_delay, self.delay_pos, mid_delay_time1);
            let mid_delayed2 = get_delayed(&self.mid_delay, self.delay_pos, mid_delay_time2);

            self.mid_delay[self.delay_pos] = mid;

            let processed_mid = mid * 0.5 + (mid_delayed1 + mid_delayed2) * 0.25;

            // --- Side processing ---
            self.side_lfo1.set_freq(self.params.side_speed.smoothed.next());
            self.side_lfo2.set_freq(self.params.side_speed.smoothed.next() * 1.05);

            let side_depth_samples = self.params.side_depth.smoothed.next() * self.sample_rate * 0.02;

            let side_delay_time1 = side_depth_samples * (1.0 + self.side_lfo1.next(self.sample_rate));
            let side_delay_time2 = side_depth_samples * (1.0 + self.side_lfo2.next(self.sample_rate));

            let side_delayed1 = get_delayed(&self.side_delay, self.delay_pos, side_delay_time1);
            let side_delayed2 = get_delayed(&self.side_delay, self.delay_pos, side_delay_time2);

            self.side_delay[self.delay_pos] = side;

            let processed_side = side * 0.5 + (side_delayed1 + side_delayed2) * 0.25;

            // --- M/S decoding and output ---
            *left = processed_mid + processed_side;
            *right = processed_mid - processed_side;

            self.delay_pos = (self.delay_pos + 1) % MAX_DELAY_SAMPLES;
        }

        ProcessStatus::Normal
    }
}

// Helper function for linear interpolation
fn get_delayed(buffer: &[f32], delay_pos: usize, delay_time_samples: f32) -> f32 {
    let read_pos = (delay_pos as f32 - delay_time_samples + buffer.len() as f32) % buffer.len() as f32;
    let read_pos_frac = read_pos.fract();
    let read_pos_int = read_pos.trunc() as usize;

    let sample1 = buffer[read_pos_int];
    let sample2 = buffer[(read_pos_int + 1) % buffer.len()];

    sample1 + (sample2 - sample1) * read_pos_frac
}

impl ClapPlugin for MidSideChorus {
    const CLAP_ID: &'static str = "com.jules.mid-side-chorus";
    const CLAP_DESCRIPTION: Option<&'static str> = Some("A mid/side chorus effect.");
    const CLAP_MANUAL_URL: Option<&'static str> = Some(Self::URL);
    const CLAP_SUPPORT_URL: Option<&'static str> = None;
    const CLAP_FEATURES: &'static [ClapFeature] = &[
        ClapFeature::AudioEffect,
        ClapFeature::Stereo,
        ClapFeature::Utility,
    ];
}

impl Vst3Plugin for MidSideChorus {
    const VST3_CLASS_ID: [u8; 16] = *b"JulesMidSideCho!";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] =
        &[Vst3SubCategory::Fx, Vst3SubCategory::Modulation];
}

nih_export_clap!(MidSideChorus);
nih_export_vst3!(MidSideChorus);
