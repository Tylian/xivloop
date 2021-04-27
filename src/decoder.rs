use std::{collections::HashMap, fs::File};

use anyhow::{Context, bail};
use lewton::inside_ogg::OggStreamReader;

use crate::CliOpts;

#[derive(Debug)]
pub struct DecodedFile {
    pub samples: (Vec<i16>, Vec<i16>),
    pub loop_start: usize,
    pub loop_end: usize,
    pub frequency: u32
}

/// Reads an OGG file and decodes the requested layer, returning the samples as well as some metadata
pub fn decode_ogg(layer: usize, opts: &CliOpts) -> anyhow::Result<DecodedFile> {
    let input_file = File::open(&opts.input).context("Count not open input file, does it exist?")?;
    let mut srr = OggStreamReader::new(input_file).context("Could not create decoder")?;

    let comment_map = srr.comment_hdr.comment_list.iter().cloned().collect::<HashMap<_, _>>();
    let loop_start = comment_map.get("LoopStart").map_or(Ok(0), |v| v.parse().context("LoopStart is not a number"))?;
    let loop_end = comment_map.get("LoopEnd").map_or(Ok(0), |v| v.parse().context("LoopEnd is not a number"))?;

    let channels = srr.ident_hdr.audio_channels as usize;
    let frequency = srr.ident_hdr.audio_sample_rate;

    let mut left_samples = Vec::new();
    let mut right_samples = Vec::new();

    if channels == 1 && layer > 1 {
        bail!("This file is mono channel, I can only encode layer 1 and you asked for {}!", layer + 1);
    } else if layer * 2 >= channels {
        bail!("This file only has {} layer(s), when you asked to encode layer {}!", channels / 2, layer + 1);
    };

    while let Some(packet) = srr.read_dec_packet().context("Could not read packet")? {
        // special case: mono
        let (left, right) = match channels {
            1 => (&packet[0], &packet[0]),
            _ => (&packet[layer], &packet[layer + 1])
        };

        /*let interleaved = left.iter()
            .zip(right.iter())
            .flat_map(|(a, b)| iter::once(a).chain(iter::once(b)))
            .copied();*/

        left_samples.extend(left);
        right_samples.extend(right);
    }

    Ok(DecodedFile { samples: (left_samples, right_samples), loop_start, loop_end, frequency })
}