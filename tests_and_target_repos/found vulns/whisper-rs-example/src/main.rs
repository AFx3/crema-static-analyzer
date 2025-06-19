use hound::WavReader;
use std::ffi::CStr;
use std::path::Path;
use std::sync::mpsc::{self, Sender};
use std::thread;
use whisper_rs::whisper_rs_sys;
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

// whisper-rs example, but the ggml-metal support is not valid? compile manully

struct SegmentMessage {
    t0: i64,
    t1: i64,
    text: String,
}

fn read_wav_to_f32(path: &Path) -> Result<Vec<f32>, String> {
    let mut reader = WavReader::open(path).expect("failed to open WAV file");
    let spec = reader.spec();
    let samples: Vec<f32> = match spec.bits_per_sample {
        16 => reader
            .samples::<i16>()
            .map(|s| s.unwrap() as f32 / i16::MAX as f32)
            .collect(),
        _ => return Err("failed to read samples".into()),
    };

    Ok(samples)
}

unsafe extern "C" fn callback(
    _: *mut whisper_rs_sys::whisper_context,
    state: *mut whisper_rs_sys::whisper_state,
    _: std::os::raw::c_int,
    user_data: *mut std::ffi::c_void,
) {
    let num_segments = whisper_rs_sys::whisper_full_n_segments_from_state(state) - 1;
    let ret = whisper_rs_sys::whisper_full_get_segment_text_from_state(state, num_segments);
    let c_str = CStr::from_ptr(ret);
    let r_str = c_str.to_str().unwrap();
    let t0 = whisper_rs_sys::whisper_full_get_segment_t0_from_state(state, num_segments);
    let t1 = whisper_rs_sys::whisper_full_get_segment_t1_from_state(state, num_segments);

    let sender = &*(user_data as *mut Sender<SegmentMessage>);

    let message = SegmentMessage {
        t0,
        t1,
        text: r_str.to_string(),
    };

    sender.send(message).unwrap();
}

fn main() {
    let (tx, rx) = mpsc::channel::<SegmentMessage>();

    thread::spawn(move || {
        for message in rx {
            println!("[{} -> {}]: {}", message.t0, message.t1, message.text);
        }
    });
    let sender_ptr = Box::into_raw(Box::new(tx)) as *mut std::ffi::c_void;
    let path_to_model = "/Users/bruce/Download/ggml-model-whisper-large-q5_0.bin";

    let ctx = WhisperContext::new_with_params(&path_to_model, WhisperContextParameters::default())
        .expect("failed to load model");

    let mut params = FullParams::new(SamplingStrategy::default());
    params.set_initial_prompt("experience");
    params.set_language(Some("auto"));

    unsafe {
        params.set_new_segment_callback(Some(callback));
        params.set_new_segment_callback_user_data(sender_ptr);
    }

    let wav_path = Path::new("/Users/bruce/python/llm/whisper/output.wav");

    let audio_data = read_wav_to_f32(wav_path).expect("failed to read audio data");

    let mut state = ctx.create_state().expect("failed to create state");
    state
        .full(params, &audio_data[..])
        .expect("failed to run model");
}
