use anyhow::{Context, anyhow};
use lame_sys::{self as ffi};
use std::error::Error;
use std::io::{Read, Write};
use std::ptr;

use libc::{c_int, c_short, size_t};

// TODO there's a safer way to do this, for sure, but it works so i'm not touching it lmao
// probably need to make my own Lame impl, or even use the lame crate?
pub fn encode_mp3<R: Read, W: Write>(reader: &mut R, writer: &mut W) -> Result<(), Box<dyn Error>> {
    let mut in_buf = [0u8; 8192 << 4]; // 8192 samples per channel (i16 samples, 2 channels)
    let mut out_buf = [0u8; 48160]; // (8192 << 4 >> 2) * 1.25 + 7200
    let mut out_vec = vec![0u8; 0];

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

    while let Ok(bytes_read) = reader.read(&mut in_buf) {
        if bytes_read == 0 {
            break;
        }
        let written = unsafe {
            ffi::lame_encode_buffer_interleaved(
                lame,
                in_buf.as_mut_ptr() as *mut c_short,
                (bytes_read >> 2) as c_int,
                out_buf.as_mut_ptr(),
                out_buf.len() as c_int,
            )
        };
        out_vec.extend_from_slice(&out_buf[..written as usize]);
    }

    let written = unsafe {
        ffi::lame_encode_flush(lame, out_buf.as_mut_ptr(), out_buf.len() as c_int)
    };
    out_vec.extend_from_slice(&out_buf[..written as usize]);

    let written = unsafe {
        ffi::lame_get_lametag_frame(lame, out_buf.as_mut_ptr(), out_buf.len() as size_t)
    };

    writer.write(&out_buf[..written as usize]).context("Failed to write lame header")?;
    writer.write(out_vec.as_slice()).context("Failed to write lame header")?;

    unsafe { ffi::lame_close(lame) };

    Ok(())
}