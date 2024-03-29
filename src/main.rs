use anyhow::{anyhow, Context, Result};
use clap::Clap;
use std::error::Error;
use std::fs::File;
use std::io::{self, BufRead, Write};
use std::path::PathBuf;

mod decoder;
mod encoder;
use crate::decoder::{decode_ogg, DecodedFile};
use crate::encoder::encode_mp3;

#[derive(Clap, Debug)]
/// Converts raw OGG files extracted from Final Fantasy XIV into playable MP3 files, optinally looping and fading the audio out
pub struct CliOpts {
    #[clap(parse(from_os_str))]
    /// Input OGG file to be looped
    input: PathBuf,

    #[clap(parse(from_os_str))]
    /// Output path, will be created if it doesn't exist
    output: PathBuf,

    #[clap(short, long)]
    /// Automagically name the output file based on input file, will treat <output> as a path rather than a file
    automatic_name: bool,

    #[clap(short, long = "no-process", parse(from_flag = std::ops::Not::not))]
    /// Do not process the file; will not loop and fade out the audio
    process: bool,

    #[clap(short, long)]
    /// Answer yes to all prompts
    yes: bool,

    #[clap(short, long, default_value = "1")]
    /// Layer to loop, starts at 1
    layer: usize,

    #[clap(short, long, default_value = "10")]
    /// Fade out duration, in seconds
    fade: usize,

    #[clap(short = 'r', long, default_value = "2")]
    /// Number of times to loop before fading out
    loops: usize,
}

fn prompt(prompt: &str, default: bool) -> Result<bool> {
    let stdin = io::stdin();
    let mut input = String::new();
    loop {
        print!("{} [{}] ", prompt, if default { "Y/n" } else { "y/N" });
        io::stdout().flush().context("Unable to flush stdout")?;
        stdin.lock().read_line(&mut input).context("Unable to read stdin")?;
        match input.to_string().to_lowercase().trim() {
            "y" | "yes" => return Ok(true),
            "n" | "no" => return Ok(false),
            "" => return Ok(default),
            _ => println!("Invalid input, expecting [y/yes/n/no] or an empty line."),
        }
    }
}

fn process_samples(decoded: &DecodedFile, opts: &CliOpts) -> (Vec<i16>, Vec<i16>) {
    let loop_start = decoded.loop_start;
    let loop_end = decoded.loop_end;
    let loop_length = loop_end - loop_start;

    let fade_length = opts.fade * decoded.frequency as usize;

    let process_channel = |samples: &[i16]| {
        // intro samples
        let intro = samples[..loop_start].iter();

        // looping body samples
        let body = samples[loop_start..loop_end].iter()
            .cycle()
            .take(loop_length * opts.loops);

        // fading outro samples
        let outro = samples[loop_start..(loop_start + fade_length)].iter()
            .enumerate()
            .map(|(i, &sample)| {
                let scale = 1.0 - i as f64 / fade_length as f64;
                (sample as f64 * scale) as i16
            });

        // optimization: working on references and then copying at the end seems to yield better performance
        intro
            .chain(body)
            .copied()
            .chain(outro)
            .collect::<Vec<i16>>()
    };

    let (left, right) = &decoded.samples;

    (process_channel(left), process_channel(right))
}

fn main() -> Result<(), Box<dyn Error>> {
    let mut opts = CliOpts::parse();

    // If automatic naming, generate the output file name, replacing the extension with ogg
    if opts.automatic_name {
        let mut output_path = opts.output.clone();
        let mut output_file = opts.input.file_stem()
            .ok_or_else(|| anyhow!("Input filename is invalid"))?
            .to_str().unwrap()
            .to_string();

        output_file.push_str(".mp3");
        output_path.push(output_file);

        opts.output = output_path;
    }

    if opts.output.exists() && !opts.yes {
        let friendly_name = opts.output.file_name().and_then(|f| f.to_str()).unwrap();
        let prompt_str = format!("The file {} already exists", friendly_name);
        if prompt(&prompt_str, true)? {
            return Err(anyhow!("File already exists, could not overwrite.").into());
        }
    }

    println!(
        "Encoding layer {} of {}...",
        (opts.layer),
        opts.input.file_name().unwrap().to_str().unwrap()
    );


    let mut decoded = decode_ogg(opts.layer - 1, &opts)?;

    // Catch both error states and states where loop start and end are 0
    let should_process = opts.process && decoded.loop_end >= decoded.loop_start;
    let (left, right) = if should_process {
        process_samples(&mut decoded, &opts)
    } else {
        decoded.samples
    };

    let mut output_file = File::create(&opts.output).with_context(|| format!("Could not open output file {:?} for writing", &opts.output))?;

    // pcm now contains the full pcm data
    encode_mp3(&left, &right, &mut output_file)?;

    Ok(())
}
