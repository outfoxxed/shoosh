use std::mem;

use pulse::{
	context::{self, Context},
	def::BufferAttr,
	mainloop::standard::{IterateResult, Mainloop},
	proplist::{self, Proplist},
	sample::{Format, Spec},
	stream::{self, PeekResult, SeekMode, Stream},
};

fn main() {
	let spec = Spec {
		format: Format::F32le,
		channels: 2,
		rate: 44100,
	};
	assert!(spec.is_valid());

	let mut proplist = Proplist::new().unwrap();
	proplist
		.set_str(proplist::properties::APPLICATION_NAME, "Shoosh")
		.unwrap();

	let mut mainloop = Mainloop::new().expect("Failed to create mainloop");

	let mut context = Context::new_with_proplist(&mainloop, "Shoosh", &proplist)
		.expect("Failed to create context");

	context
		.connect(None, context::FlagSet::NOFLAGS, None)
		.expect("Failed to connect to pulseaudio");

	let poll_mainloop = |mainloop: &mut Mainloop| match mainloop.iterate(false) {
		IterateResult::Err(_) | IterateResult::Quit(_) => {
			eprintln!("Iterate unsuccessful, exiting...");
			return
		}
		IterateResult::Success(_) => {}
	};

	// wait for context
	loop {
		poll_mainloop(&mut mainloop);

		match context.get_state() {
			context::State::Ready => break,
			context::State::Failed | context::State::Terminated => {
				eprintln!("Context state is failed or terminated, exiting...");
				return
			}
			_ => {}
		}
	}

	let mut playback_stream = Stream::new(&mut context, "Shoosh sink", &spec, None)
		.expect("Failed to create playback stream");

	let mut recording_stream = Stream::new(&mut context, "Shoosh source", &spec, None)
		.expect("Failed to create recording stream");

	playback_stream
		.connect_playback(
			None,
			Some(&BufferAttr {
				maxlength: u32::MAX,
				tlength: 1024,
				prebuf: u32::MAX,
				minreq: u32::MAX,
				fragsize: 0,
			}),
			stream::FlagSet::empty(),
			None,
			None,
		)
		.expect("Failed to connect playback stream");

	recording_stream
		.connect_record(
			None,
			Some(&BufferAttr {
				maxlength: u32::MAX,
				tlength: 0,
				prebuf: 0,
				minreq: 0,
				fragsize: 1024 * mem::size_of::<f32>() as u32,
			}),
			stream::FlagSet::empty(),
		)
		.expect("Failed to connect recording stream");

	// wait for streams
	'wait_streams: loop {
		poll_mainloop(&mut mainloop);

		for stream in [&playback_stream, &recording_stream] {
			match stream.get_state() {
				stream::State::Ready => {}
				stream::State::Failed | stream::State::Terminated => {
					eprintln!("Stream state is failed or terminated, exiting...");
					return
				}
				_ => continue 'wait_streams,
			}
		}

		break
	}

	loop {
		poll_mainloop(&mut mainloop);

		match recording_stream.peek().unwrap() {
			PeekResult::Empty => {}
			PeekResult::Hole(_) => recording_stream.discard().unwrap(),
			PeekResult::Data(data) => {
				let start = std::time::Instant::now();
				let ichunks = data
					.chunks(mem::size_of::<f32>())
					.map(|chunk| f32::from_le_bytes(<[u8; 4]>::try_from(chunk).unwrap()))
					.collect::<Vec<f32>>();
				let audio_iter = ichunks
					.chunks(1024)
					.map(|data| {
						let avg = data.iter().fold(0.0, |a: f32, &b| a.max(b));
						let mul = if avg > 0.01 { 0.01 / avg } else { 1.0 };
						data.iter().map(move |d| d * mul)
					})
					.flatten();
				let audio_data = Vec::from_iter(audio_iter);
				let avg = audio_data
					.iter()
					.map(|f| f.abs())
					.fold(0.0, |a: f32, b| a.max(b));

				println!("Vec size: {}, avg volume: {avg:?}", data.len() / 4);

				playback_stream
					.write(
						&audio_data
							.iter()
							.map(|f| f.to_le_bytes())
							.flatten()
							.collect::<Vec<_>>()[..],
						None,
						0,
						SeekMode::Relative,
					)
					.unwrap();

				recording_stream.discard().unwrap();
				println!("Processing took {:?}", std::time::Instant::now().duration_since(start));
			}
		}
	}
}
