use std::{env, mem, num::ParseFloatError};

use getopts::Options;
use pulse::{
	context::{self, Context},
	def::BufferAttr,
	mainloop::standard::{IterateResult, Mainloop},
	proplist::{self, Proplist},
	sample::{Format, Spec},
	stream::{self, PeekResult, SeekMode, Stream},
};

use crate::ringbuffer::RingBuffer;

mod ringbuffer;

fn main() {
	let args = env::args().collect::<Vec<_>>();

	let mut opts = Options::new();
	opts.optflag("h", "help", "print this help");
	opts.optopt(
		"v",
		"volume",
		"maximum allowable volume,\nplay with it until you get it right, probably somewhere \
		 between 0.0 and 1.0",
		"VOLUME",
	);
	let matches = match opts.parse(&args[1..]) {
		Ok(x) => x,
		Err(e) => {
			println!("{}", e.to_string());
			return
		}
	};

	if matches.opt_present("h") {
		print!("{}", opts.usage(&format!("usage: {} [options]", args[0].to_owned())));
		return
	}

	let volume_cap = match matches.opt_get::<f32>("v") {
		Ok(None) => {
			println!("volume cap must be specified (-v)");
			return
		}
		Err(ParseFloatError { .. }) => {
			println!("volume cap must be a decimal value");
			return
		}
		Ok(Some(x)) => x,
	};

	println!("Volume cap: {volume_cap}");

	run(volume_cap);
}

fn run(volume_cap: f32) {
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

	let poll_mainloop = |mainloop: &mut Mainloop| match mainloop.iterate(true) {
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

	const BUFFER_SIZE: usize = 128;
	let mut volume_buffer = RingBuffer::<f32>::new(BUFFER_SIZE);
	loop {
		poll_mainloop(&mut mainloop);

		match recording_stream.peek().unwrap() {
			PeekResult::Empty => {}
			PeekResult::Hole(_) => recording_stream.discard().unwrap(),
			PeekResult::Data(data) => {
				let start = std::time::Instant::now();
				let float_data = data
					.chunks(mem::size_of::<f32>())
					.map(|chunk| f32::from_le_bytes(<[u8; 4]>::try_from(chunk).unwrap()))
					.collect::<Vec<f32>>();
				let audio_data = float_data
					.chunks(64)
					.map(|chunk| {
						let chunk_max = chunk
							.iter()
							.fold(0.0, |a: f32, &b| f32::max(a.abs(), b.abs()));
						volume_buffer.append(&[chunk_max]);

						let weighted_average = volume_buffer
							.iter()
							.enumerate()
							.map(|(i, v)| v * (i as f32 / BUFFER_SIZE as f32))
							.sum::<f32>() / (BUFFER_SIZE as f32 * 0.5);

						let volume_multiplier =
							volume_cap / weighted_average.max(volume_cap).max(chunk_max);
						/*println!(
							"VolMul: {volume_multiplier:.03} | WAVG: {weighted_average:.3} | \
							 CWAVG: {:.3}",
							weighted_average.max(volume_cap).max(chunk_max)
						);*/
						chunk.into_iter().map(move |v| v * volume_multiplier)
					})
					.flatten()
					.collect::<Vec<_>>();

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
