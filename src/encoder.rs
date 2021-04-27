use anyhow::{Context, anyhow};
use lame_sys::{self as ffi};
use std::error::Error;
use std::io::Write;
use std::ptr;

use libc::{c_int, size_t};

const BUF_SIZE: usize = 8192 << 4; // 8192 samples per channel (i16 samples, 2 channels)
const OUT_BUF_SIZE: usize = BUF_SIZE * 5 / 4 + 7200; // (BUF_SIZE >> 2) * 1.25 + 7200

// TODO there's a safer way to do this, for sure, but it works so i'm not touching it lmao
// probably need to make my own Lame impl, or even use the lame crate?
pub fn encode_mp3(left: &[i16], right: &[i16], writer: &mut impl Write) -> Result<(), Box<dyn Error>> {
    assert_eq!(left.len(), right.len(), "Left and right channels must be equal length");

    let lame = unsafe { ffi::lame_init() };
    if lame == ptr::null_mut() {
        return Err(anyhow!("Could not create LAME context").into());
    }

    unsafe {
        ffi::lame_set_num_channels(lame, 2);
        ffi::lame_set_mode(lame, ffi::MPEG_mode::JOINT_STEREO);
        ffi::lame_set_VBR(lame, ffi::vbr_mode::vbr_mtrh);
        ffi::lame_set_preset(lame, ffi::preset_mode::V2 as libc::c_int);
        ffi::lame_init_params(lame);
    }

    let mut out_buf = [0u8; OUT_BUF_SIZE]; 
    let mut out_vec = left.to_owned().chunks_mut(BUF_SIZE)
        .zip(right.to_owned().chunks_mut(BUF_SIZE))
        .fold(vec![0u8; 0], |mut acc, (left_chunk, right_chunk)| {
            let written = unsafe {
                ffi::lame_encode_buffer(
                    lame,
                    left_chunk.as_mut_ptr(),
                    right_chunk.as_mut_ptr(),
                    left_chunk.len() as c_int,
                    out_buf.as_mut_ptr(),
                    OUT_BUF_SIZE as c_int,
                )
            };
    
            if written < 0 {
                panic!("lame_encode_buffer returned {}", written);
            }

            acc.extend_from_slice(&out_buf[..written as usize]);
            acc
        });

    let written = unsafe { ffi::lame_encode_flush(lame, out_buf.as_mut_ptr(), out_buf.len() as c_int) };
    out_vec.extend_from_slice(&out_buf[..written as usize]);

    let written = unsafe { ffi::lame_get_lametag_frame(lame, out_buf.as_mut_ptr(), out_buf.len() as size_t) };
    writer.write(&out_buf[..written as usize]).context("Failed to write lame header")?;
    writer.write(out_vec.as_slice()).context("Failed to write lame header")?;

    unsafe { ffi::lame_close(lame) };

    Ok(())
}