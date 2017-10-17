extern crate byteorder;
#[macro_use]
extern crate clap;
extern crate lame_sys;
extern crate libc;
extern crate vorbis;

use byteorder::{ByteOrder, LittleEndian};
use clap::App;
use lame_sys::*;
use std::fs::File;
use std::io;
use std::io::Cursor;
use std::io::prelude::*;
use std::path::Path;
use timers::Timer;
use vorbis::Decoder;

mod timers;

fn encode_mp3<R: Read, W: Write + Seek>(reader: &mut R, writer: &mut W) {
    let mut in_buf = [0u8; 8192 << 4]; // 8192 samples per channel (i16 samples, 2 channels)
    let mut out_buf = [0u8; 48160]; // (8192 << 4 >> 2) * 1.25 + 7200
    let mut out_vec = vec![0u8; 0];

    let lame = unsafe { lame_init() };

    unsafe {
        lame_set_num_channels(lame, 2);
        lame_set_mode(lame, MPEG_mode::JOINT_STEREO);
        lame_set_preset(lame, preset_mode::V2 as libc::c_int);
        lame_init_params(lame);
    }

    while let Ok(bytes_read) = reader.read(&mut in_buf) {
        if bytes_read == 0 {
            break;
        }

        let written = unsafe {
            lame_encode_buffer_interleaved(
                lame,
                in_buf.as_mut_ptr() as *mut libc::c_short,
                (bytes_read >> 2) as libc::c_int,
                out_buf.as_mut_ptr(),
                out_buf.len() as libc::c_int,
            )
        };
        out_vec.extend_from_slice(&out_buf[..written as usize]);
    }

    let written =
        unsafe { lame_encode_flush(lame, out_buf.as_mut_ptr(), out_buf.len() as libc::c_int) };
    out_vec.extend_from_slice(&out_buf[..written as usize]);

    let written = unsafe {
        lame_get_lametag_frame(lame, out_buf.as_mut_ptr(), out_buf.len() as libc::size_t)
    };

    writer.write(&out_buf[..written as usize]).expect(
        "Failed to write lame header",
    );
    writer.write(out_vec.as_slice()).expect(
        "Failed to write output",
    );
}

fn prompt(prompt: &str, default: bool) -> bool {
    let default_text = match default {
        true => "Y/n",
        false => "y/N",
    };

    let stdin = io::stdin();
    let mut handle = stdin.lock();
    let mut input = String::new();
    loop {
        print!("{} [{}] ", prompt, default_text);
        io::stdout().flush().ok().expect("Could not flush stdout");
        handle.read_line(&mut input).ok().expect(
            "Error reading line",
        );
        match input.to_string().to_lowercase().trim() {
            "y" | "yes" => return true,
            "n" | "no" => return false,
            "" => return default,
            _ => println!("Invalid input, expecting [y/yes/n/no] or an empty line."),
        }
    }
}

fn main() {
    let mut timer = Timer::new();
    timer.start("program");

    let yml = load_yaml!("cli.yml");
    let matches = App::from_yaml(yml).get_matches();

    let input_path = Path::new(matches.value_of("INPUT").unwrap());
    let output_path = Path::new(matches.value_of("OUTPUT").unwrap());

    if output_path.exists() && !matches.is_present("yes") {
        if !prompt("Output file exists. Overwrite?", true) {
            println!("Okay. Bye!");
            std::process::exit(0);
        }
    }

    println!(
        "Encoding {}...",
        input_path.file_name().unwrap().to_str().unwrap()
    );

    let input_file = File::open(input_path).unwrap_or_else(|e| {
        println!("Count not open input file: {}", e);
        std::process::exit(1);
    });

    let mut output_file = File::create(output_path).unwrap_or_else(|e| {
        println!("Count not open output file: {}", e);
        std::process::exit(1);
    });

    if cfg!(debug_assertions) {
        timer.start("comments")
    };

    let mut decoder = Decoder::new(input_file).unwrap();
    let mut loop_start = 0;
    let mut loop_end = 0;

    let layer: usize = matches.value_of("layer").unwrap().parse::<usize>().expect(
        "Layer is not a number",
    ) - 1;
    let fade: usize = matches.value_of("fade").unwrap().parse().expect(
        "Fade duration is not a number",
    );
    let loops: usize = matches.value_of("loops").unwrap().parse().expect(
        "Loop count is not a number",
    );

    for comment in decoder.comments().iter() {
        if comment.starts_with("LoopStart=") {
            loop_start = comment[10..].parse().expect("LoopStart is not a number!");
        } else if comment.starts_with("LoopEnd=") {
            loop_end = comment[8..].parse().expect("LoopEnd is not a number!");
        }
    }

    if cfg!(debug_assertions) {
        timer.report("comments")
    };

    // Catch both error states and states where loop start and end are 0
    let process = if loop_end <= loop_start {
        false
    } else {
        !matches.is_present("no-process")
    };

    let mut source = Vec::new();
    let mut frequency = 0;

    if cfg!(debug_assertions) {
        timer.start("ogg")
    };
    for p in decoder.packets() {
        if let Ok(packet) = p {
            frequency = packet.rate;

            source.reserve(packet.data.len() / packet.channels as usize * 2);

            // special case: mono
            if packet.channels == 1 {
                for sample in packet.data {
                    source.push(sample);
                    source.push(sample);
                }
            } else {
                if layer >= packet.channels as usize / 2 {
                    println!(
                        "This file only has {} layer(s), when you asked to encode layer {}!",
                        packet.channels / 2,
                        layer + 1
                    );
                    std::process::exit(1);
                };

                for i in 0..packet.data.len() / packet.channels as usize {
                    let index = i * packet.channels as usize + (layer as usize * 2);
                    source.push(packet.data[index]);
                    source.push(packet.data[index + 1]);
                }
            }
        }
    }

    if cfg!(debug_assertions) {
        timer.report("ogg");
        timer.start("loop")
    };

    let samples = if process {
        let loop_start = loop_start * 2;
        let loop_end = loop_end * 2;
        let loop_length = loop_end - loop_start;

        let fade_length = fade * frequency as usize * 2;

        let mut pcm = vec![0i16; loop_start + loop_length * loops + fade_length];

        // intro
        pcm[0..loop_start].copy_from_slice(&source[..loop_start]);

        // loops
        for i in 0..loops {
            let slice_start = loop_start + loop_length * i;
            let slice_end = loop_start + loop_length * i + loop_length;
            pcm[slice_start..slice_end].copy_from_slice(&source[loop_start..loop_end]);
        }

        // fade
        if fade_length > 0 {
            let fade = &mut source[loop_start..(loop_start + fade_length)];
            for i in 0..(fade_length >> 1) {
                let scale = 1.0 - i as f64 / (fade_length >> 1) as f64;
                fade[i * 2] = (fade[i * 2] as f64 * scale) as i16;
                fade[i * 2 + 1] = (fade[i * 2 + 1] as f64 * scale) as i16;
            }

            let slice_start = loop_start + loop_length * loops;
            let slice_end = loop_start + loop_length * loops + fade_length;
            pcm[slice_start..slice_end].copy_from_slice(&fade);
        }

        pcm
    } else {
        source
    };

    if cfg!(debug_assertions) {
        timer.report("loop");
        timer.start("transmute")
    };

    let mut pcm = vec![0u8; samples.len() * 2];
    LittleEndian::write_i16_into(&samples, &mut pcm);

    if cfg!(debug_assertions) {
        timer.report("transmute")
    };

    // pcm now contains the full pcm data
    if cfg!(debug_assertions) {
        timer.start("mp3")
    };

    encode_mp3(&mut Cursor::new(pcm), &mut output_file);
    if cfg!(debug_assertions) {
        timer.report("mp3")
    };

    timer.report_with("program", |elapsed| {
        println!(
            "Encoded {} in {:.3}s",
            output_path.file_name().unwrap().to_str().unwrap(),
            elapsed.unwrap()
        );
    });
}
