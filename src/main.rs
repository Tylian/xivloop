extern crate lame_sys;
extern crate vorbis;
extern crate libc;
extern crate time;
extern crate clap;

use clap::{App, Arg};

use std::fs::File;
use std::path::Path;
use std::io::prelude::*;
use std::io::Cursor;

use std::io;

use lame_sys::*;
use vorbis::Decoder;

fn encode_mp3<R, W>(reader: &mut R, writer: &mut W)
    where R: Read, W: Write {

    let mut in_buf = [0u8; 8192 << 4]; // 8192 samples per channel (i16 samples, 2 channels)
    let mut out_buf = [0u8; 0x7fff]; // swear to fuk

    let lame = unsafe { lame_init() };

    unsafe {
        lame_set_in_samplerate(lame, 44100);
        lame_set_VBR(lame, lame_sys::vbr_default);
        lame_set_VBR_quality(lame, 6.0);
        lame_init_params(lame);
    }

    while let Ok(bytes_read) = reader.read(&mut in_buf) {
        if bytes_read == 0 { break }

        let write = unsafe {
            lame_encode_buffer_interleaved(
                lame,
                in_buf.as_mut_ptr() as *mut i16,
                (bytes_read >> 2) as i32,
                out_buf.as_mut_ptr(),
                out_buf.len() as i32,
            )
        };

        writer.write(&out_buf[..write as usize]).unwrap();
    }

    let write = unsafe {
        lame_encode_flush(
            lame,
            out_buf.as_mut_ptr(),
            out_buf.len() as i32
        )
    };
    writer.write(&out_buf[..write as usize]).unwrap();
}

fn interleave_channels(left_channel: &[i16], right_channel: &[i16]) -> Box<[u8]> {
    let mut pcm = vec![0u8; (left_channel.len() + right_channel.len() << 1) as usize].into_boxed_slice();
    for i in 0..left_channel.len() {
        pcm[i*4]   = (left_channel[i] & 0xff) as u8;
        pcm[i*4+1] = (left_channel[i] >> 8) as u8;
        pcm[i*4+2] = (right_channel[i] & 0xff) as u8;
        pcm[i*4+3] = (right_channel[i] >> 8) as u8;
    }

    pcm
}

fn file_exists(path: String) -> Result<(), String> {
        std::fs::metadata(path)
            .and(Ok(()))
            .or(Err(String::from("File doesn't exist")))
}

fn is_number(input: String) -> Result<(), String> {
        input.parse::<usize>()
            .and(Ok(()))
            .or(Err(String::from("Not a valid number")))
}

fn prompt(prompt: &str, default: bool) -> bool {
    let stdin = io::stdin();
    let mut input = String::new();
    loop {
        print!("{} [{}] ", prompt, if default { "Y/n" } else { "y/N" });
        io::stdout().flush().ok().expect("Could not flush stdout");
        stdin.lock().read_line(&mut input).ok().expect("Error reading line");
        match input.trim_right_matches("\n").to_string().to_lowercase().trim() {
            "y" | "yes" => { return true },
            "n" | "no"  => { return false },
            ""          => { return default },
            _           => { println!("Invalid input, expecting [y/yes/n/no] or an empty line.") }
        }
    }
}

fn main() {
    let start = time::precise_time_s();
    let matches = App::new("xivloop")
        .version("1.0")
        .author("Tylian <me@tylian.net>")
        .about("Loops Final Fantasy XIV .scd.ogg files")
        .arg(Arg::with_name("layer").short("l").long("layer").value_name("NUMBER").help("Layer number to loop, 1 is first, etc.").takes_value(true).validator(is_number).default_value("1"))
        .arg(Arg::with_name("loops").short("r").long("loops").value_name("NUMBER").help("Number of times to loop").takes_value(true).validator(is_number).default_value("2"))
        .arg(Arg::with_name("fade").short("f").long("fade").value_name("SECONDS").help("Fade out duration").takes_value(true).validator(is_number).default_value("10"))
        .arg(Arg::with_name("yes").short("y").long("assume-yes").help("Assume yes to all prompts"))
        .arg(Arg::with_name("dont_loop").long("dont-loop").help("Do not loop, just output file"))
        .arg(Arg::with_name("INPUT").required(true).help("Input file to process").validator(file_exists))
        .arg(Arg::with_name("OUTPUT").required(true).help("Output file location"))
    .get_matches();

    let input_path = Path::new(matches.value_of("INPUT").unwrap());
    let output_path = Path::new(matches.value_of("OUTPUT").unwrap());

    if output_path.exists() && !matches.is_present("yes") {
        if !prompt("Output file exists. Overwrite?", true) {
            println!("Okay. Bye!");
           std::process::exit(0);
        }
    }
    
    // holy shit path lol
    println!("Encoding {}...", input_path.file_name().unwrap().to_str().unwrap());
    
    let file = match File::open(input_path) {
        Ok(_f) => _f,
        Err(e) => {
            println!("Error opening input file!\n{}", e);
            std::process::exit(1);
        }
    };

    let mut out = match File::create(output_path) {
        Ok(_f) => _f,
        Err(e) => {
            println!("Count not open output file!\n{}", e);
            std::process::exit(1);
        }
    };
    
    let mut decoder = Decoder::new(file).unwrap();

    let mut loop_start = 0;
    let mut loop_end = 0;

    let layer:usize = matches.value_of("layer").unwrap().parse::<usize>().unwrap() - 1;
    let fade:usize = matches.value_of("fade").unwrap().parse().unwrap();
    let loops:usize = matches.value_of("loops").unwrap().parse().unwrap();
    let mut dont_loop = matches.is_present("dont_loop");

    for comment in decoder.comments().iter() {
        if comment.starts_with("LoopStart=") {
            loop_start = comment[10..].parse().unwrap();
        } else if comment.starts_with("LoopEnd=") {
            loop_end = comment[8..].parse().unwrap();
        }
    }

    if loop_start == 0 || loop_end == 0 {
        dont_loop = true;
    }

    let loop_length = loop_end - loop_start;

    let mut original_left = Vec::new();
    let mut original_right = Vec::new();

    let mut frequency = 0;

    for p in decoder.packets() {
        if let Ok(packet) = p {
            frequency = packet.rate;

            original_left.reserve(packet.data.len() / packet.channels as usize);
            original_right.reserve(packet.data.len() / packet.channels as usize);

            // special case: mono
            if packet.channels == 1 {
                original_left.extend(packet.data.clone());
                original_right.extend(packet.data.clone());
            } else {
                if layer >= packet.channels as usize / 2 {
                    println!("This file only has {} layer(s), when you asked to encode layer {}!", packet.channels / 2, layer + 1);
                    std::process::exit(1);
                };

                for i in 0..packet.data.len() / packet.channels as usize {
                    original_left.push(packet.data[i * packet.channels as usize + (layer as usize * 2) ]);
                    original_right.push(packet.data[i * packet.channels as usize + (layer as usize * 2) + 1 ])
                }
            }
        }
    }

    let pcm = if !dont_loop {
        let fade_length:usize = fade * frequency as usize;

        let mut left_channel = vec![0i16; loop_start + loop_length * loops + fade_length].into_boxed_slice();
        let mut right_channel = vec![0i16; loop_start + loop_length * loops + fade_length].into_boxed_slice();

        // intro
        left_channel[0..loop_start].copy_from_slice(&original_left[..loop_start]);
        right_channel[0..loop_start].copy_from_slice(&original_right[..loop_start]);

        // loops
        for i in 0..loops {
            let slice_start = loop_start + loop_length * i;
            let slice_end = loop_start + loop_length * i + loop_length;
            left_channel[slice_start..slice_end].copy_from_slice(&original_left[loop_start..loop_end]);
            right_channel[slice_start..slice_end].copy_from_slice(&original_right[loop_start..loop_end]);
        }

        // fade
        if fade_length > 0 {
            let left_fade = &mut original_left[loop_start..(loop_start + fade_length)];
            let right_fade = &mut original_right[loop_start..(loop_start + fade_length)];

            // cut off one sample so I don't divide by 0 lol
            for i in 0..fade_length  {
                let scale = 1.0 - i as f64 / fade_length as f64;
                left_fade[i] = (left_fade[i] as f64 * scale) as i16;
                right_fade[i] = (right_fade[i] as f64 * scale) as i16;
            }

            let slice_start = loop_start + loop_length * loops;
            let slice_end = loop_start + loop_length * loops + fade_length;
            left_channel[slice_start..slice_end].copy_from_slice(&left_fade);
            right_channel[slice_start..slice_end].copy_from_slice(&right_fade);
        }
        
        interleave_channels(&left_channel, &right_channel)
    } else {
        let left_channel = original_left.into_boxed_slice();
        let right_channel = original_right.into_boxed_slice();

        interleave_channels(&left_channel, &right_channel)
    };

    // pcm now contains the full pcm data
    encode_mp3(&mut Cursor::new(pcm), &mut out);
    println!("Encoded {} in {:.3}s", output_path.file_name().unwrap().to_str().unwrap(), time::precise_time_s() - start);
}
