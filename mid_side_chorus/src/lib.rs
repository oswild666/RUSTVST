#[macro_use]
extern crate vst;

use std::f32::consts::PI;
use std::sync::Arc;
use vst::buffer::AudioBuffer;
use vst::plugin::{Category, Info, Plugin, PluginParameters};
use vst::util::AtomicFloat;

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

struct Parameters {
    mid_depth: AtomicFloat,
    mid_speed: AtomicFloat,
    side_depth: AtomicFloat,
    side_speed: AtomicFloat,
}

impl Default for Parameters {
    fn default() -> Self {
        Self {
            mid_depth: AtomicFloat::new(0.5),
            mid_speed: AtomicFloat::new(0.5),
            side_depth: AtomicFloat::new(0.5),
            side_speed: AtomicFloat::new(0.5),
        }
    }
}

impl PluginParameters for Parameters {
    fn get_parameter(&self, index: i32) -> f32 {
        match index {
            0 => self.mid_depth.get(),
            1 => self.mid_speed.get(),
            2 => self.side_depth.get(),
            3 => self.side_speed.get(),
            _ => 0.0,
        }
    }

    fn set_parameter(&self, index: i32, value: f32) {
        match index {
            0 => self.mid_depth.set(value),
            1 => self.mid_speed.set(value),
            2 => self.side_depth.set(value),
            3 => self.side_speed.set(value),
            _ => (),
        }
    }

    fn get_parameter_text(&self, index: i32) -> String {
        format!("{:.2}", self.get_parameter(index))
    }

    fn get_parameter_name(&self, index: i32) -> String {
        match index {
            0 => "Mid Depth".to_string(),
            1 => "Mid Speed".to_string(),
            2 => "Side Depth".to_string(),
            3 => "Side Speed".to_string(),
            _ => "".to_string(),
        }
    }
}

struct MidSideChorus {
    params: Arc<Parameters>,
    sample_rate: f32,
    mid_lfo1: Lfo,
    mid_lfo2: Lfo,
    side_lfo1: Lfo,
    side_lfo2: Lfo,
    mid_delay: Vec<f32>,
    side_delay: Vec<f32>,
    delay_pos: usize,
}

impl Plugin for MidSideChorus {
    fn new(_host: vst::host::Host) -> Self {
        Self {
            params: Arc::new(Parameters::default()),
            sample_rate: 44100.0,
            mid_lfo1: Lfo::new(0.5),
            mid_lfo2: Lfo::new(0.51),
            side_lfo1: Lfo::new(0.6),
            side_lfo2: Lfo::new(0.61),
            mid_delay: vec![0.0; MAX_DELAY_SAMPLES],
            side_delay: vec![0.0; MAX_DELAY_SAMPLES],
            delay_pos: 0,
        }
    }

    fn get_info(&self) -> Info {
        Info {
            name: "Mid Side Chorus".to_string(),
            vendor: "Jules".to_string(),
            unique_id: 13371337,
            category: Category::Effect,
            inputs: 2,
            outputs: 2,
            parameters: 4,
            ..Default::default()
        }
    }

    fn set_sample_rate(&mut self, rate: f32) {
        self.sample_rate = rate;
    }

    fn resume(&mut self) {
        self.mid_lfo1.phase = 0.0;
        self.mid_lfo2.phase = 0.0;
        self.side_lfo1.phase = 0.0;
        self.side_lfo2.phase = 0.0;
        self.mid_delay.fill(0.0);
        self.side_delay.fill(0.0);
        self.delay_pos = 0;
    }

    fn get_parameter_object(&mut self) -> Arc<dyn PluginParameters> {
        self.params.clone()
    }

    fn process(&mut self, buffer: &mut AudioBuffer<f32>) {
        let (inputs, outputs) = buffer.split();
        let (in_l, in_r) = inputs.split_at(1);
        let (mut out_l, mut out_r) = outputs.split_at_mut(1);
        let (in_l, in_r) = (&in_l[0], &in_r[0]);
        let (out_l, out_r) = (&mut out_l[0], &mut out_r[0]);

        for i in 0..buffer.samples() {
            let left = in_l[i];
            let right = in_r[i];

            let mid = (left + right) * 0.5;
            let side = (left - right) * 0.5;

            // --- Mid processing ---
            self.mid_lfo1.set_freq(self.params.mid_speed.get() * 10.0);
            self.mid_lfo2.set_freq(self.params.mid_speed.get() * 10.0 * 1.05);

            let mid_depth_samples = self.params.mid_depth.get() * self.sample_rate * 0.02;

            let mid_delay_time1 = mid_depth_samples * (1.0 + self.mid_lfo1.next(self.sample_rate));
            let mid_delay_time2 = mid_depth_samples * (1.0 + self.mid_lfo2.next(self.sample_rate));

            let mid_delayed1 = get_delayed(&self.mid_delay, self.delay_pos, mid_delay_time1);
            let mid_delayed2 = get_delayed(&self.mid_delay, self.delay_pos, mid_delay_time2);

            self.mid_delay[self.delay_pos] = mid;

            let processed_mid = mid * 0.5 + (mid_delayed1 + mid_delayed2) * 0.25;

            // --- Side processing ---
            self.side_lfo1.set_freq(self.params.side_speed.get() * 10.0);
            self.side_lfo2.set_freq(self.params.side_speed.get() * 10.0 * 1.05);

            let side_depth_samples = self.params.side_depth.get() * self.sample_rate * 0.02;

            let side_delay_time1 = side_depth_samples * (1.0 + self.side_lfo1.next(self.sample_rate));
            let side_delay_time2 = side_depth_samples * (1.0 + self.side_lfo2.next(self.sample_rate));

            let side_delayed1 = get_delayed(&self.side_delay, self.delay_pos, side_delay_time1);
            let side_delayed2 = get_delayed(&self.side_delay, self.delay_pos, side_delay_time2);

            self.side_delay[self.delay_pos] = side;

            let processed_side = side * 0.5 + (side_delayed1 + side_delayed2) * 0.25;

            // --- M/S decoding and output ---
            out_l[i] = processed_mid + processed_side;
            out_r[i] = processed_mid - processed_side;

            self.delay_pos = (self.delay_pos + 1) % MAX_DELAY_SAMPLES;
        }
    }
}

fn get_delayed(buffer: &[f32], delay_pos: usize, delay_time_samples: f32) -> f32 {
    let read_pos = (delay_pos as f32 - delay_time_samples + buffer.len() as f32) % buffer.len() as f32;
    let read_pos_frac = read_pos.fract();
    let read_pos_int = read_pos.trunc() as usize;

    let sample1 = buffer[read_pos_int];
    let sample2 = buffer[(read_pos_int + 1) % buffer.len()];

    sample1 + (sample2 - sample1) * read_pos_frac
}

plugin_main!(MidSideChorus);
