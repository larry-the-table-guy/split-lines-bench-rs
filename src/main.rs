// only enable avx512 for x86-64 nightly builds
#![cfg_attr(
    all(feature = "nightly", target_arch = "x86_64"),
    feature(avx512_target_feature)
)]
#![cfg_attr(
    all(feature = "nightly", target_arch = "x86_64"),
    feature(stdarch_x86_avx512)
)]

mod slice {
    pub fn std(input: &str) -> Vec<&str> {
        input.lines().collect()
    }

    pub fn std_reuse<'input>(input: &'input str, out: &mut Vec<&'input str>) {
        for line in input.lines() {
            out.push(line);
        }
    }

    #[cfg(target_arch = "x86_64")]
    pub mod x86_64 {
        use std::arch::x86_64::*;

        pub fn sse2<'input>(input: &'input str, out: &mut Vec<&'input str>) {
            // scan 16-byte chunks, then handle tail
            let mut line_start = 0;
            unsafe {
                let nl_v = _mm_loadu_si128([b'\n'; 16].as_ptr().cast());
                for (chunk_i, chunk) in input.as_bytes().chunks_exact(16).enumerate() {
                    let v = _mm_loadu_si128(chunk.as_ptr().cast());
                    let mut mask = _mm_movemask_epi8(_mm_cmpeq_epi8(v, nl_v)) as u16;
                    while mask != 0 {
                        /*
                        abcdefNhijklNmoN
                        (reversed, so first char is lowest bit)
                        1001000001000000
                         */
                        let bit_pos = mask.trailing_zeros() as usize;
                        let line_end = chunk_i * 16 + bit_pos;
                        out.push(&input[line_start..line_end]);
                        line_start = line_end + 1;
                        mask &= mask - 1;
                    }
                }
            }
            tail(line_start, 16, input, out);
        }

        fn tail<'input>(
            mut line_start: usize,
            chunk_size: usize,
            input: &'input str,
            out: &mut Vec<&'input str>,
        ) {
            // handle last bytes
            for i in (input.len() & !(chunk_size - 1))..input.len() {
                if input.as_bytes()[i] != b'\n' {
                    continue;
                }
                out.push(unsafe { input.get_unchecked(line_start..i) });
                line_start = i + 1;
            }
            // handle last line. omit if empty
            if line_start != input.len() {
                out.push(unsafe { input.get_unchecked(line_start..) });
            }
        }

        pub fn sse2_unsafe<'input>(input: &'input str, out: &mut Vec<&'input str>) {
            // scan 16-byte chunks, then handle tail
            let mut line_start = 0;
            unsafe {
                let nl_v = _mm_loadu_si128([b'\n'; 16].as_ptr().cast());
                for (chunk_i, chunk) in input.as_bytes().chunks_exact(16).enumerate() {
                    let v = _mm_loadu_si128(chunk.as_ptr().cast());
                    let mut mask = _mm_movemask_epi8(_mm_cmpeq_epi8(v, nl_v)) as u16;
                    while mask != 0 {
                        let bit_pos = mask.trailing_zeros() as usize;
                        let line_end = chunk_i * 16 + bit_pos;
                        out.push(input.get_unchecked(line_start..line_end));
                        line_start = line_end + 1;
                        mask &= mask - 1;
                    }
                }
            }
            tail(line_start, 16, input, out);
        }

        pub fn sse2_unroll<'input>(input: &'input str, out: &mut Vec<&'input str>) {
            // Key idea is to pull the allocation out of the innermost loop

            let mut line_start = 0;
            unsafe {
                let nl_v = _mm_loadu_si128([b'\n'; 16].as_ptr().cast());
                let mut chunk_i = 0;
                let stop_chunk_i = input.len() / 16;
                while chunk_i < stop_chunk_i {
                    let mut write_i = 0;
                    out.reserve(256);
                    let out_arr = out.spare_capacity_mut().get_unchecked_mut(..256);
                    while write_i < (256 - 16) && chunk_i < stop_chunk_i {
                        let v = _mm_loadu_si128(input.as_ptr().byte_add(chunk_i * 16).cast());
                        let mut mask = _mm_movemask_epi8(_mm_cmpeq_epi8(v, nl_v)) as u16;
                        while mask != 0 {
                            let bit_pos = mask.trailing_zeros() as usize;
                            let line_end = chunk_i * 16 + bit_pos;
                            out_arr
                                .get_unchecked_mut(write_i)
                                .write(input.get_unchecked(line_start..line_end));
                            write_i += 1;
                            line_start = line_end + 1;
                            mask &= mask - 1;
                        }
                        chunk_i += 1;
                    }
                    out.set_len(out.len() + write_i);
                }
            }
            tail(line_start, 16, input, out);
        }

        pub fn sse2_unrollx4<'input>(input: &'input str, out: &mut Vec<&'input str>) {
            let mut line_start = 0;
            unsafe {
                let nl_v = _mm_loadu_si128([b'\n'; 16].as_ptr().cast());
                let mut chunk_i = 0;
                let stop_chunk_i = input.len() / 64;
                while chunk_i < stop_chunk_i {
                    let mut write_i = 0;
                    out.reserve(256);
                    let out_arr = out.spare_capacity_mut().get_unchecked_mut(..256);
                    while write_i < (256 - 64) && chunk_i < stop_chunk_i {
                        use std::arch::x86_64::{
                            _mm_cmpeq_epi8 as eq, _mm_loadu_si128 as load,
                            _mm_movemask_epi8 as movemask,
                        };
                        let in_ptr = input.as_ptr().byte_add(chunk_i * 64).cast::<__m128i>();
                        let mask0 = movemask(eq(load(in_ptr), nl_v)) as u64;
                        let mask1 = movemask(eq(load(in_ptr.byte_add(16)), nl_v)) as u64;
                        let mask2 = movemask(eq(load(in_ptr.byte_add(32)), nl_v)) as u64;
                        let mask3 = movemask(eq(load(in_ptr.byte_add(48)), nl_v)) as u64;
                        let mut mask = mask0 | (mask1 << 16) | (mask2 << 32) | (mask3 << 48);
                        while mask != 0 {
                            let bit_pos = mask.trailing_zeros() as usize;
                            let line_end = chunk_i * 64 + bit_pos;
                            out_arr
                                .get_unchecked_mut(write_i)
                                .write(input.get_unchecked(line_start..line_end));
                            write_i += 1;
                            line_start = line_end + 1;
                            mask &= mask - 1;
                        }
                        chunk_i += 1;
                    }
                    out.set_len(out.len() + write_i);
                }
            }
            tail(line_start, 64, input, out);
        }

        pub fn can_run_avx2() -> bool {
            is_x86_feature_detected!("avx2")
                && is_x86_feature_detected!("bmi1")
                && is_x86_feature_detected!("popcnt")
        }

        #[target_feature(enable = "avx2,bmi1,popcnt")]
        pub unsafe fn avx2<'input>(input: &'input str, out: &mut Vec<&'input str>) {
            // scan 32-byte chunks, then handle tail
            let mut line_start = 0;
            let nl_v = _mm256_loadu_si256([b'\n'; 32].as_ptr().cast());
            for (chunk_i, chunk) in input.as_bytes().chunks_exact(32).enumerate() {
                let v = _mm256_loadu_si256(chunk.as_ptr().cast());
                let mut mask = _mm256_movemask_epi8(_mm256_cmpeq_epi8(v, nl_v)) as u32;
                while mask != 0 {
                    let bit_pos = mask.trailing_zeros() as usize;
                    let line_end = chunk_i * 32 + bit_pos;
                    out.push(&input[line_start..line_end]);
                    line_start = line_end + 1;
                    mask &= mask - 1;
                }
            }
            tail(line_start, 32, input, out);
        }

        #[target_feature(enable = "avx2,bmi1,popcnt")]
        pub unsafe fn avx2_unsafe<'input>(input: &'input str, out: &mut Vec<&'input str>) {
            // scan 32-byte chunks, then handle tail
            let mut line_start = 0;
            let nl_v = _mm256_loadu_si256([b'\n'; 32].as_ptr().cast());
            for (chunk_i, chunk) in input.as_bytes().chunks_exact(32).enumerate() {
                let v = _mm256_loadu_si256(chunk.as_ptr().cast());
                let mut mask = _mm256_movemask_epi8(_mm256_cmpeq_epi8(v, nl_v)) as u32;
                while mask != 0 {
                    let bit_pos = mask.trailing_zeros() as usize;
                    let line_end = chunk_i * 32 + bit_pos;
                    out.push(input.get_unchecked(line_start..line_end));
                    line_start = line_end + 1;
                    mask &= mask - 1;
                }
            }
            tail(line_start, 32, input, out);
        }

        #[target_feature(enable = "avx2,bmi1,popcnt")]
        pub unsafe fn avx2_unroll<'input>(input: &'input str, out: &mut Vec<&'input str>) {
            // Key idea is to pull the allocation out of the innermost loop
            let mut line_start = 0;
            let nl_v = _mm256_loadu_si256([b'\n'; 32].as_ptr().cast());
            let mut chunk_i = 0;
            let stop_chunk_i = input.len() / 32;
            while chunk_i < stop_chunk_i {
                let mut write_i = 0;
                // this is the only function call in the loop. Vector registers have to be reloaded
                // after a function call. That's why we go through the trouble of removing it from the
                // inner loop.
                out.reserve(256);
                let out_arr = out.spare_capacity_mut().get_unchecked_mut(..256);
                // at most 32 items will be added per chunk
                while write_i <= (256 - 32) && chunk_i < stop_chunk_i {
                    let v = _mm256_loadu_si256(input.as_ptr().byte_add(chunk_i * 32).cast());
                    let mut mask = _mm256_movemask_epi8(_mm256_cmpeq_epi8(v, nl_v)) as u32;
                    while mask != 0 {
                        let bit_pos = mask.trailing_zeros() as usize;
                        let line_end = chunk_i * 32 + bit_pos;
                        out_arr
                            .get_unchecked_mut(write_i)
                            .write(input.get_unchecked(line_start..line_end));
                        write_i += 1;
                        line_start = line_end + 1;
                        mask &= mask - 1;
                    }
                    chunk_i += 1;
                }
                out.set_len(out.len() + write_i);
            }
            tail(line_start, 32, input, out);
        }

        #[target_feature(enable = "avx2,bmi1,popcnt")]
        pub unsafe fn avx2_unrollx2<'input>(input: &'input str, out: &mut Vec<&'input str>) {
            use std::arch::x86_64::{
                _mm256_cmpeq_epi8 as eq, _mm256_loadu_si256 as load,
                _mm256_movemask_epi8 as movemask,
            };
            let mut line_start = 0;
            let nl_v = _mm256_loadu_si256([b'\n'; 32].as_ptr().cast());
            let mut chunk_i = 0;
            let stop_chunk_i = input.len() / 64;
            while chunk_i < stop_chunk_i {
                let mut write_i = 0;
                // this is the only function call in the loop. Vector registers have to be reloaded
                // after a function call. That's why we go through the trouble of removing it from the
                // inner loop.
                out.reserve(256);
                let out_arr = out.spare_capacity_mut().get_unchecked_mut(..256);
                // at most 64 items will be added per chunk
                while write_i <= (256 - 64) && chunk_i < stop_chunk_i {
                    let ptr = input.as_ptr().byte_add(chunk_i * 64);
                    let v1 = load(ptr.cast());
                    let v2 = load(ptr.byte_add(32).cast());
                    let mut mask = ((movemask(eq(v2, nl_v)) as u32 as u64) << 32)
                        | (movemask(eq(v1, nl_v)) as u32 as u64);
                    while mask != 0 {
                        let bit_pos = mask.trailing_zeros() as usize;
                        let line_end = chunk_i * 64 + bit_pos;
                        out_arr
                            .get_unchecked_mut(write_i)
                            .write(input.get_unchecked(line_start..line_end));
                        write_i += 1;
                        line_start = line_end + 1;
                        mask &= mask - 1;
                    }
                    chunk_i += 1;
                }
                out.set_len(out.len() + write_i);
            }
            tail(line_start, 64, input, out);
        }
    }
}

mod compressed {
    #[derive(PartialEq, Eq)]
    pub struct LineIndex {
        /// Low 16 bits of each newline's index
        /// One per line.
        pub lows: Vec<u16>,
        /// d[i] is the first index into 'lows' where the high bits are i
        /// One per 64KB of input.
        pub high_starts: Vec<usize>,
    }

    pub fn iter(input: &str, out: &mut LineIndex) {
        for chunk in input.as_bytes().chunks(1 << 16) {
            out.high_starts.push(out.lows.len());
            for (idx, _) in chunk.iter().enumerate().filter(|e| *e.1 == b'\n') {
                out.lows.push(idx as u16);
            }
        }
    }

    /// Assumes high_start has already been written
    pub fn tail(chunk_size: usize, input: &str, out: &mut LineIndex) {
        let base = input.len() & !(chunk_size - 1);
        for (idx, _) in input.as_bytes()[base..]
            .iter()
            .enumerate()
            .filter(|e| *e.1 == b'\n')
        {
            out.lows.push(base as u16 + idx as u16);
        }
    }

    #[cfg(target_arch = "x86_64")]
    pub mod x86_64 {
        use crate::compressed::*;
        use std::arch::x86_64::*;

        pub fn sse2(input: &str, out: &mut LineIndex) {
            let nl_v = unsafe { _mm_loadu_si128([b'\n'; 16].as_ptr().cast()) };
            for chunk_64k in input.as_bytes().chunks(1 << 16) {
                out.high_starts.push(out.lows.len());
                for (chunk_idx, chunk) in chunk_64k.chunks_exact(16).enumerate() {
                    unsafe {
                        let v = _mm_loadu_si128(chunk.as_ptr().cast());
                        let mut mask = _mm_movemask_epi8(_mm_cmpeq_epi8(v, nl_v)) as u16;
                        while mask != 0 {
                            let bit_pos = mask.trailing_zeros() as u16;
                            out.lows.push(chunk_idx as u16 * 16 + bit_pos);
                            mask &= mask - 1;
                        }
                    }
                }
            }
            tail(16, input, out);
        }

        pub fn sse2_unroll(input: &str, out: &mut LineIndex) {
            let nl_v = unsafe { _mm_loadu_si128([b'\n'; 16].as_ptr().cast()) };
            for chunk_64k in input.as_bytes().chunks(1 << 16) {
                out.high_starts.push(out.lows.len());
                let mut chunk_i = 0;
                let stop_chunk_i = chunk_64k.len() / 16;
                while chunk_i < stop_chunk_i {
                    let mut write_i = 0;
                    out.lows.reserve(256);
                    unsafe {
                        let out_arr = out.lows.spare_capacity_mut().get_unchecked_mut(..256);
                        while write_i <= (256 - 16) && chunk_i < stop_chunk_i {
                            let v = _mm_loadu_si128(chunk_64k.as_ptr().add(chunk_i * 16).cast());
                            let mut mask = _mm_movemask_epi8(_mm_cmpeq_epi8(v, nl_v)) as u16;
                            while mask != 0 {
                                let bit_pos = mask.trailing_zeros() as u16;
                                out_arr
                                    .get_unchecked_mut(write_i)
                                    .write(chunk_i as u16 * 16 + bit_pos);
                                write_i += 1;
                                mask &= mask - 1;
                            }
                            chunk_i += 1;
                        }
                        out.lows.set_len(out.lows.len() + write_i);
                    }
                }
            }
            tail(16, input, out);
        }

        pub fn sse2_unrollx4(input: &str, out: &mut LineIndex) {
            use std::arch::x86_64::{
                _mm_cmpeq_epi8 as eq, _mm_loadu_si128 as load, _mm_movemask_epi8 as movemask,
            };
            let nl_v = unsafe { load([b'\n'; 16].as_ptr().cast()) };
            for chunk_64k in input.as_bytes().chunks(1 << 16) {
                out.high_starts.push(out.lows.len());
                let mut chunk_i = 0;
                let stop_chunk_i = chunk_64k.len() / 64;
                while chunk_i < stop_chunk_i {
                    let mut write_i = 0;
                    out.lows.reserve(256);
                    unsafe {
                        let out_arr = out.lows.spare_capacity_mut().get_unchecked_mut(..256);
                        while write_i <= (256 - 64) && chunk_i < stop_chunk_i {
                            let in_ptr =
                                chunk_64k.as_ptr().byte_add(chunk_i * 64).cast::<__m128i>();
                            let mask0 = movemask(eq(load(in_ptr), nl_v)) as u64;
                            let mask1 = movemask(eq(load(in_ptr.byte_add(16)), nl_v)) as u64;
                            let mask2 = movemask(eq(load(in_ptr.byte_add(32)), nl_v)) as u64;
                            let mask3 = movemask(eq(load(in_ptr.byte_add(48)), nl_v)) as u64;
                            let mut mask = mask0 | (mask1 << 16) | (mask2 << 32) | (mask3 << 48);
                            while mask != 0 {
                                let bit_pos = mask.trailing_zeros() as u16;
                                out_arr
                                    .get_unchecked_mut(write_i)
                                    .write(chunk_i as u16 * 64 + bit_pos);
                                write_i += 1;
                                mask &= mask - 1;
                            }
                            chunk_i += 1;
                        }
                        out.lows.set_len(out.lows.len() + write_i);
                    }
                }
            }
            tail(64, input, out);
        }

        pub fn can_run_sse42() -> bool {
            is_x86_feature_detected!("popcnt")
        }

        // enabling bmi1 isn't interesting bc there's a very narrow slice of CPUs with BMI1 but not
        // AVX2, but a broad range of older CPUS with popcnt
        #[target_feature(enable = "popcnt")]
        pub unsafe fn sse42_unrollx4_interleavex2(input: &str, out: &mut LineIndex) {
            use std::arch::x86_64::{
                _mm_cmpeq_epi8 as eq, _mm_loadu_si128 as load, _mm_movemask_epi8 as movemask,
            };
            const CHUNK_SIZE: usize = 128;
            /// count_ones() without branching on the zero case. Result undefined if input is 0
            /// Same encoding as tzcnt.
            fn rep_bsf(input: u64) -> u64 {
                let mut output;
                unsafe {
                    std::arch::asm!("rep bsf {output}, {input}", input = in(reg) input, output = out(reg) output)
                };
                output
            }
            let nl_v = unsafe { load([b'\n'; 16].as_ptr().cast()) };
            for chunk_64k in input.as_bytes().chunks(1 << 16) {
                out.high_starts.push(out.lows.len());
                let mut chunk_i = 0;
                let stop_chunk_i = chunk_64k.len() / CHUNK_SIZE;
                while chunk_i < stop_chunk_i {
                    let mut write_i = 0;
                    let iter_count = 32.min(stop_chunk_i - chunk_i);
                    out.lows.reserve(iter_count * CHUNK_SIZE);
                    let out_arr = out
                        .lows
                        .spare_capacity_mut()
                        .get_unchecked_mut(..iter_count * CHUNK_SIZE);
                    for _ in 0..iter_count {
                        let mut mask1 = {
                            let in_ptr = chunk_64k
                                .as_ptr()
                                .byte_add(chunk_i * CHUNK_SIZE)
                                .cast::<__m128i>();
                            let mask0 = movemask(eq(load(in_ptr), nl_v)) as u64;
                            let mask1 = movemask(eq(load(in_ptr.byte_add(16)), nl_v)) as u64;
                            let mask2 = movemask(eq(load(in_ptr.byte_add(32)), nl_v)) as u64;
                            let mask3 = movemask(eq(load(in_ptr.byte_add(48)), nl_v)) as u64;
                            mask0 | (mask1 << 16) | (mask2 << 32) | (mask3 << 48)
                        };

                        let mut mask2 = {
                            let in_ptr = chunk_64k
                                .as_ptr()
                                .byte_add(chunk_i * CHUNK_SIZE + 64)
                                .cast::<__m128i>();
                            let mask0 = movemask(eq(load(in_ptr), nl_v)) as u64;
                            let mask1 = movemask(eq(load(in_ptr.byte_add(16)), nl_v)) as u64;
                            let mask2 = movemask(eq(load(in_ptr.byte_add(32)), nl_v)) as u64;
                            let mask3 = movemask(eq(load(in_ptr.byte_add(48)), nl_v)) as u64;
                            mask0 | (mask1 << 16) | (mask2 << 32) | (mask3 << 48)
                        };
                        let mut write_i2 = write_i + mask1.count_ones() as usize;
                        let mask2_count = mask2.count_ones() as usize;

                        while mask1 != 0 {
                            let bit_pos = mask1.trailing_zeros() as u16;
                            out_arr
                                .get_unchecked_mut(write_i)
                                .write(chunk_i as u16 * CHUNK_SIZE as u16 + bit_pos);
                            write_i += 1;
                            mask1 &= mask1 - 1;

                            let bit_pos = rep_bsf(mask2) as u16;
                            out_arr.get_unchecked_mut(write_i2).write(
                                (chunk_i as u16 * CHUNK_SIZE as u16)
                                    .wrapping_add(64)
                                    .wrapping_add(bit_pos),
                            );
                            write_i2 += 1;
                            mask2 &= mask2.wrapping_sub(1);
                        }
                        write_i += mask2_count;
                        while mask2 != 0 {
                            let bit_pos = mask2.trailing_zeros() as u16;
                            out_arr
                                .get_unchecked_mut(write_i2)
                                .write(chunk_i as u16 * CHUNK_SIZE as u16 + 64 + bit_pos);
                            write_i2 += 1;
                            mask2 &= mask2 - 1;
                        }
                        chunk_i += 1;
                    }
                    out.lows.set_len(out.lows.len() + write_i);
                }
            }
            tail(128, input, out);
        }

        pub fn can_run_avx2() -> bool {
            // in practice, avx2 also implies bmi1 and popcnt
            is_x86_feature_detected!("avx2")
                && is_x86_feature_detected!("bmi1")
                && is_x86_feature_detected!("popcnt")
        }

        #[target_feature(enable = "avx2,bmi1,popcnt")]
        pub unsafe fn avx2_unroll(input: &str, out: &mut LineIndex) {
            let nl_v = unsafe { _mm256_loadu_si256([b'\n'; 32].as_ptr().cast()) };
            for chunk_64k in input.as_bytes().chunks(1 << 16) {
                out.high_starts.push(out.lows.len());
                let mut chunk_i = 0;
                let stop_chunk_i = chunk_64k.len() / 32;
                while chunk_i < stop_chunk_i {
                    let mut write_i = 0;
                    out.lows.reserve(256);
                    let out_arr = out.lows.spare_capacity_mut().get_unchecked_mut(..256);
                    while write_i <= (256 - 32) && chunk_i < stop_chunk_i {
                        let v = _mm256_loadu_si256(chunk_64k.as_ptr().add(chunk_i * 32).cast());
                        let mut mask = _mm256_movemask_epi8(_mm256_cmpeq_epi8(v, nl_v)) as u32;
                        while mask != 0 {
                            let bit_pos = mask.trailing_zeros() as u16;
                            out_arr
                                .get_unchecked_mut(write_i)
                                .write(chunk_i as u16 * 32 + bit_pos);
                            write_i += 1;
                            mask &= mask - 1;
                        }
                        chunk_i += 1;
                    }
                    out.lows.set_len(out.lows.len() + write_i);
                }
            }
            tail(32, input, out);
        }

        #[target_feature(enable = "avx2,bmi1,popcnt")]
        pub unsafe fn avx2_unrollx2(input: &str, out: &mut LineIndex) {
            use std::arch::x86_64::{
                _mm256_cmpeq_epi8 as eq, _mm256_loadu_si256 as load,
                _mm256_movemask_epi8 as movemask,
            };
            let nl_v = unsafe { _mm256_loadu_si256([b'\n'; 32].as_ptr().cast()) };
            for chunk_64k in input.as_bytes().chunks(1 << 16) {
                out.high_starts.push(out.lows.len());
                let mut chunk_i = 0;
                let stop_chunk_i = chunk_64k.len() / 64;
                while chunk_i < stop_chunk_i {
                    let mut write_i = 0;
                    out.lows.reserve(256);
                    let out_arr = out.lows.spare_capacity_mut().get_unchecked_mut(..256);
                    while write_i <= (256 - 64) && chunk_i < stop_chunk_i {
                        let ptr = chunk_64k.as_ptr().add(chunk_i * 64);
                        let v1 = load(ptr.cast());
                        let v2 = load(ptr.byte_add(32).cast());
                        let mut mask = ((movemask(eq(v2, nl_v)) as u32 as u64) << 32)
                            | (movemask(eq(v1, nl_v)) as u32 as u64);
                        while mask != 0 {
                            let bit_pos = mask.trailing_zeros() as u16;
                            out_arr
                                .get_unchecked_mut(write_i)
                                .write(chunk_i as u16 * 64 + bit_pos);
                            write_i += 1;
                            mask &= mask - 1;
                        }
                        chunk_i += 1;
                    }
                    out.lows.set_len(out.lows.len() + write_i);
                }
            }
            tail(64, input, out);
        }

        #[target_feature(enable = "avx2,bmi1,popcnt")]
        pub unsafe fn avx2_unrollx2_interleavex2(input: &str, out: &mut LineIndex) {
            use std::arch::x86_64::{
                _mm256_cmpeq_epi8 as eq, _mm256_loadu_si256 as load,
                _mm256_movemask_epi8 as movemask,
            };
            const CHUNK_SIZE: usize = 128;
            let nl_v = unsafe { _mm256_loadu_si256([b'\n'; 32].as_ptr().cast()) };
            for chunk_64k in input.as_bytes().chunks(1 << 16) {
                out.high_starts.push(out.lows.len());
                let mut chunk_i = 0;
                let stop_chunk_i = chunk_64k.len() / CHUNK_SIZE;
                while chunk_i < stop_chunk_i {
                    // two iters of 64B, start 2nd at + popcount, stop when first exhausted,
                    // finish 2nd
                    let mut write_i = 0;
                    let iter_count = 32.min(stop_chunk_i - chunk_i);
                    out.lows.reserve(iter_count * CHUNK_SIZE);
                    let out_arr = out
                        .lows
                        .spare_capacity_mut()
                        .get_unchecked_mut(..iter_count * CHUNK_SIZE);
                    for _ in 0..iter_count {
                        let ptr = chunk_64k.as_ptr().add(chunk_i * CHUNK_SIZE);
                        let v1 = load(ptr.cast());
                        let v2 = load(ptr.byte_add(32).cast());
                        let mut mask1 = ((movemask(eq(v2, nl_v)) as u32 as u64) << 32)
                            | (movemask(eq(v1, nl_v)) as u32 as u64);

                        let v1 = load(ptr.byte_add(64).cast());
                        let v2 = load(ptr.byte_add(96).cast());
                        let mut mask2 = ((movemask(eq(v2, nl_v)) as u32 as u64) << 32)
                            | (movemask(eq(v1, nl_v)) as u32 as u64);
                        let mut write_i2 = write_i + mask1.count_ones() as usize;
                        let mask2_count = mask2.count_ones() as usize;
                        while mask1 != 0 {
                            let bit_pos = mask1.trailing_zeros() as u16;
                            out_arr
                                .get_unchecked_mut(write_i)
                                .write(chunk_i as u16 * CHUNK_SIZE as u16 + bit_pos);
                            write_i += 1;
                            mask1 &= mask1 - 1;

                            let bit_pos = _tzcnt_u64(mask2) as u16;
                            // if this turns out to be a junk value, it will be ignored later (by
                            // truncating the slice). So, overflowing is fine.
                            out_arr.get_unchecked_mut(write_i2).write(
                                (chunk_i as u16 * CHUNK_SIZE as u16)
                                    .wrapping_add(64)
                                    .wrapping_add(bit_pos),
                            );
                            write_i2 += 1;
                            mask2 &= mask2.wrapping_sub(1);
                        }
                        write_i += mask2_count;
                        while mask2 != 0 {
                            let bit_pos = mask2.trailing_zeros() as u16;
                            out_arr
                                .get_unchecked_mut(write_i2)
                                .write(chunk_i as u16 * CHUNK_SIZE as u16 + 64 + bit_pos);
                            write_i2 += 1;
                            mask2 &= mask2 - 1;
                        }
                        chunk_i += 1;
                    }
                    out.lows.set_len(out.lows.len() + write_i);
                }
            }
            tail(128, input, out);
        }

        #[cfg(feature = "nightly")]
        pub fn can_run_avx512_compress() -> bool {
            is_x86_feature_detected!("popcnt")
                && is_x86_feature_detected!("avx512f")
                && is_x86_feature_detected!("avx512bw")
                && is_x86_feature_detected!("avx512vbmi2")
        }

        #[inline(never)]
        #[cfg(feature = "nightly")]
        #[target_feature(enable = "popcnt,avx512f,avx512bw,avx512vbmi2")]
        pub unsafe fn avx512_compress(input: &str, out: &mut LineIndex) {
            const IDX_ARR: [u8; 64] = {
                let mut t = [0u8; 64];
                let mut i = 0;
                while i < t.len() {
                    t[i] = i as u8;
                    i += 1;
                }
                t
            };
            let nl_v = _mm512_set1_epi8(b'\n' as i8);
            let idx_v = _mm512_loadu_epi8(IDX_ARR.as_ptr().cast());
            let i16_64_v = _mm512_set1_epi16(64);
            for chunk_64k in input.as_bytes().chunks(1 << 16) {
                out.high_starts.push(out.lows.len());
                let mut offset_v = _mm512_setzero_si512();
                let mut chunk_i = 0;
                let stop_chunk_i = chunk_64k.len() / 64;
                while chunk_i < stop_chunk_i {
                    let mut write_i = 0;
                    out.lows.reserve(256);
                    let out_arr = out.lows.spare_capacity_mut().get_unchecked_mut(..256);
                    while write_i <= (256 - 64) && chunk_i < stop_chunk_i {
                        let v = _mm512_loadu_si512(chunk_64k.as_ptr().add(chunk_i * 64).cast());
                        let mask = _mm512_cmpeq_epi8_mask(v, nl_v);
                        let num_lines = mask.count_ones();
                        let idxs = _mm512_maskz_compress_epi8(mask, idx_v);
                        // first half
                        let low_idxs = _mm512_cvtepu8_epi16(_mm512_castsi512_si256(idxs));
                        let low_idxs = _mm512_add_epi16(low_idxs, offset_v);
                        _mm512_storeu_si512(out_arr.as_mut_ptr().add(write_i).cast(), low_idxs);
                        // second half
                        if num_lines > 32 {
                            let high_idxs =
                                _mm512_cvtepu8_epi16(_mm512_extracti64x4_epi64::<1>(idxs));
                            let high_idxs = _mm512_add_epi16(high_idxs, offset_v);
                            // if there are any results in high_idxs, then low must have been full, so
                            // we can unconditionally write 64 bytes ahead of the previous addr
                            _mm512_storeu_si512(
                                out_arr.as_mut_ptr().add(write_i).byte_add(64).cast(),
                                high_idxs,
                            );
                        }
                        offset_v = _mm512_add_epi16(offset_v, i16_64_v);
                        write_i += num_lines as usize;
                        chunk_i += 1;
                    }
                    out.lows.set_len(out.lows.len() + write_i);
                }
            }
            tail(64, input, out);
        }
    }
}

fn reset_vector<'b, T: ?Sized>(mut vec: Vec<&T>) -> Vec<&'b T> {
    vec.clear();
    let cap = vec.capacity();
    let ptr = vec.as_mut_ptr();
    std::mem::forget(vec);
    unsafe { Vec::from_raw_parts(ptr.cast(), 0, cap) }
}

/// M: min bytes per line, N: max bytes per line
fn prep_vec_range<const M: usize, const N: usize>(vec: &mut Vec<u8>) -> usize {
    use std::collections::HashSet; // Used to shuffle a sequence of ints
    assert!(M <= N);
    vec.fill(b'a');
    let mut idx = 0;
    (0..vec.len().min(256 * 1024 * 1024) * 2 / (N + M))
        .collect::<HashSet<usize>>()
        .iter()
        .copied()
        .map(|i| M + (i % (N - M + 1)))
        .for_each(|i| {
            idx += i;
            vec[idx] = b'\n';
        });
    vec.len().min(256 * 1024 * 1024)
}

type SliceSplitFn = for<'a, 'b> fn(&'a str, &'b mut Vec<&'a str>);
type CompressSplitFn = unsafe fn(&str, &mut compressed::LineIndex);
type FeatCheckFn = fn() -> bool;

fn main() {
    use std::hint::black_box;
    use std::time::Instant;

    let benchmark_stages: &[(&str, fn(&mut Vec<u8>) -> usize)] = &[
        ("single line", |vec| vec.len()),
        ("1-20", prep_vec_range::<1, 20>),
        ("5-20", prep_vec_range::<5, 20>),
        ("10-30", prep_vec_range::<10, 30>),
        ("0-40", prep_vec_range::<0, 40>),
        ("0-80", prep_vec_range::<0, 80>),
        ("40-120", prep_vec_range::<40, 120>),
        ("0-0", |vec| {
            vec.fill(b'\n');
            // Slices takes 16GB w/ 1 billion
            vec.len().min(64 * 1024 * 1024)
        }),
    ];
    let slice_bench_cases: &[(&str, FeatCheckFn, SliceSplitFn)] = &[
        ("std_reuse", || true, slice::std_reuse),
        #[cfg(target_arch = "x86_64")]
        ("sse2", || true, slice::x86_64::sse2),
        #[cfg(target_arch = "x86_64")]
        ("sse2_unsafe", || true, slice::x86_64::sse2_unsafe),
        #[cfg(target_arch = "x86_64")]
        ("sse2_unroll", || true, slice::x86_64::sse2_unroll),
        #[cfg(target_arch = "x86_64")]
        ("sse2_unrollx4", || true, slice::x86_64::sse2_unrollx4),
        #[cfg(target_arch = "x86_64")]
        ("avx2", slice::x86_64::can_run_avx2, |a, b| unsafe {
            slice::x86_64::avx2(a, b)
        }),
        #[cfg(target_arch = "x86_64")]
        ("avx2_unsafe", slice::x86_64::can_run_avx2, |a, b| unsafe {
            slice::x86_64::avx2_unsafe(a, b)
        }),
        #[cfg(target_arch = "x86_64")]
        ("avx2_unroll", slice::x86_64::can_run_avx2, |a, b| unsafe {
            slice::x86_64::avx2_unroll(a, b)
        }),
        #[cfg(target_arch = "x86_64")]
        (
            "avx2_unrollx2",
            slice::x86_64::can_run_avx2,
            |a, b| unsafe { slice::x86_64::avx2_unrollx2(a, b) },
        ),
    ];
    let compressed_bench_cases: &[(&str, FeatCheckFn, CompressSplitFn)] = &[
        ("iter", || true, compressed::iter),
        #[cfg(target_arch = "x86_64")]
        ("sse2", || true, compressed::x86_64::sse2),
        #[cfg(target_arch = "x86_64")]
        ("sse2 unroll", || true, compressed::x86_64::sse2_unroll),
        #[cfg(target_arch = "x86_64")]
        ("sse2 unrollx4", || true, compressed::x86_64::sse2_unrollx4),
        #[cfg(target_arch = "x86_64")]
        (
            "sse4 intrlv",
            compressed::x86_64::can_run_sse42,
            compressed::x86_64::sse42_unrollx4_interleavex2,
        ),
        #[cfg(target_arch = "x86_64")]
        (
            "avx2 unroll",
            compressed::x86_64::can_run_avx2,
            compressed::x86_64::avx2_unroll,
        ),
        #[cfg(target_arch = "x86_64")]
        (
            "avx2 unrollx2",
            compressed::x86_64::can_run_avx2,
            compressed::x86_64::avx2_unrollx2,
        ),
        #[cfg(target_arch = "x86_64")]
        (
            "avx2 intrlv",
            compressed::x86_64::can_run_avx2,
            compressed::x86_64::avx2_unrollx2_interleavex2,
        ),
        #[cfg(all(feature = "nightly", target_arch = "x86_64"))]
        (
            "avx512",
            compressed::x86_64::can_run_avx512_compress,
            compressed::x86_64::avx512_compress,
        ),
    ];

    // this can be done with Vecs, but this is fine
    let mut slice_thrpts = Vec::new();
    let mut compressed_thrpts = Vec::new();

    let mut b = vec![b'a'; 1024 * 1024 * 1024];

    // pre-fill the vec (beyond just reserving) so that the first fn doesn't pay for all the page
    // misses (some OSs may give CoW zero pages for `Vec::with_capacity(...)` )
    let mut pool_out_slice_buf = black_box(vec![""; 64 * 1024 * 1024]);
    let mut out_compressed_buf = compressed::LineIndex {
        lows: Vec::with_capacity(64 * 1024 * 1024),
        high_starts: Vec::with_capacity(16),
    };
    let mut test_compressed_buf = compressed::LineIndex {
        lows: Vec::new(),
        high_starts: Vec::new(),
    };

    for (stage_label, prep_fn) in benchmark_stages {
        println!("\n\t\t{stage_label}");
        let mut cur_slice_thrpts = Vec::new();
        let mut cur_compressed_thrpts = Vec::new();

        let len = prep_fn(&mut b);
        let input = std::str::from_utf8(&b[..len]).unwrap();
        let mut out_slice_buf = pool_out_slice_buf;

        println!("\tslices");
        {
            let start = Instant::now();
            black_box(slice::std(input));
            let duration = start.elapsed().as_secs_f64();
            let thrpt = len as f64 / duration / 1_000_000.;
            println!("{fn_label:<13}: {thrpt:>8.0}", fn_label = "std");
            cur_slice_thrpts.push(thrpt);
        }
        for (fn_label, feat_checker, fnc) in slice_bench_cases {
            if !feat_checker() {
                println!("skipping {fn_label} because of missing CPU features");
                continue;
            }
            out_slice_buf.clear();
            let start = Instant::now();
            fnc(input, &mut out_slice_buf);
            let duration = start.elapsed().as_secs_f64();
            black_box(&mut out_slice_buf);
            let thrpt = len as f64 / duration / 1_000_000.;
            println!("{fn_label:<13}: {thrpt:>8.0}");
            cur_slice_thrpts.push(thrpt);
        }
        // run first test case again to show that it's not sensitive to order (e.g. cache)
        {
            let start = Instant::now();
            black_box(slice::std(input));
            let duration = start.elapsed().as_secs_f64();
            let thrpt = len as f64 / duration / 1_000_000.;
            println!("{fn_label:<13}: {thrpt:>8.0}", fn_label = "std");
            cur_slice_thrpts.push(thrpt);
        }

        println!("\tcompressed");
        test_compressed_buf.lows.clear();
        test_compressed_buf.high_starts.clear();
        compressed::iter(input, &mut test_compressed_buf);
        for (fn_label, feat_checker, fnc) in compressed_bench_cases {
            if !feat_checker() {
                println!("skipping {fn_label} because of missing CPU features");
                continue;
            }
            out_compressed_buf.lows.clear();
            out_compressed_buf.high_starts.clear();
            let start = Instant::now();
            unsafe { fnc(input, &mut out_compressed_buf) };
            let duration = start.elapsed().as_secs_f64();
            black_box(&mut out_compressed_buf);
            let thrpt = len as f64 / duration / 1_000_000.;
            println!("{fn_label:<13}: {thrpt:>8.0}");
            cur_compressed_thrpts.push(thrpt);
            assert!(
                out_compressed_buf == test_compressed_buf,
                "(compressed) {fn_label} failed during {stage_label}"
            );
        }

        pool_out_slice_buf = reset_vector(out_slice_buf);

        slice_thrpts.push(cur_slice_thrpts);
        compressed_thrpts.push(cur_compressed_thrpts);
    }

    // now, print the markdown tables

    // Headers
    println!("\n## Slice\n");
    let print_table_header = || {
        print!("| algo |");
        for (stage_label, ..) in benchmark_stages {
            print!(" {stage_label} |");
        }
        println!();
        print!("| :-- |");
        for _ in benchmark_stages {
            print!(" --: |");
        }
        println!();
    };
    print_table_header();
    // | Algo | thrpts... |
    print!("| std |");
    for thrpt in slice_thrpts.iter().map(|vec| vec[0]) {
        print!(" {thrpt:.0} |");
    }
    println!();
    for (idx, (algo_name, ..)) in slice_bench_cases.iter().enumerate() {
        print!("| {algo_name} |");
        for thrpt in slice_thrpts.iter().map(|vec| vec[idx + 1]) {
            print!(" {thrpt:.0} |")
        }
        println!();
    }

    println!("\n## Compressed format\n");
    print_table_header();
    for (idx, (algo_name, ..)) in compressed_bench_cases.iter().enumerate() {
        print!("| {algo_name} |");
        for thrpt in compressed_thrpts.iter().map(|vec| vec[idx]) {
            print!(" {thrpt:.0} |")
        }
        println!();
    }
}

#[cfg(test)]
mod tests {
    use crate::slice::*;

    static TEST_CASES: &[(&str, &[&str])] = &[
        ("", &[]),
        ("a", &["a"]),
        ("\n", &[""]),
        ("\nab", &["", "ab"]),
        ("a\n", &["a"]),
        ("a\nbc", &["a", "bc"]),
        ("\n\n", &["", ""]),
        ("\n\n\n", &["", "", ""]),
        (
            "123\n123456\n123456789012\n",
            &["123", "123456", "123456789012"],
        ),
        (
            "12345678901234567\n12345678901234567\n12345678901234567\n",
            &[
                "12345678901234567",
                "12345678901234567",
                "12345678901234567",
            ],
        ),
    ];

    #[test]
    fn test_std() {
        for (input, expected) in TEST_CASES {
            let out = std(input);
            assert_eq!(expected, &out, "input: `{input}`");
        }
    }

    #[test]
    fn test_std_reuse() {
        let mut buf = Vec::new();
        for (input, expected) in TEST_CASES {
            buf.clear();
            std_reuse(input, &mut buf);
            assert_eq!(expected, &buf, "input: `{input}`");
        }
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn test_sse2() {
        let mut buf = Vec::new();
        for (input, expected) in TEST_CASES {
            buf.clear();
            x86_64::sse2(input, &mut buf);
            assert_eq!(expected, &buf, "input: `{input}`");
        }
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn test_sse2_unroll() {
        let mut buf = Vec::new();
        for (input, expected) in TEST_CASES {
            buf.clear();
            x86_64::sse2_unroll(input, &mut buf);
            assert_eq!(expected, &buf, "input: `{input}`");
        }
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn test_sse2_unrollx4() {
        let mut buf = Vec::new();
        for (input, expected) in TEST_CASES {
            buf.clear();
            x86_64::sse2_unrollx4(input, &mut buf);
            assert_eq!(expected, &buf, "input: `{input}`");
        }
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn test_avx2() {
        if !x86_64::can_run_avx2() {
            return;
        }
        let mut buf = Vec::new();
        for (input, expected) in TEST_CASES {
            buf.clear();
            unsafe { x86_64::avx2(input, &mut buf) };
            assert_eq!(expected, &buf, "input: `{input}`");
        }
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn test_avx2_unroll() {
        if !x86_64::can_run_avx2() {
            return;
        }
        let mut buf = Vec::new();
        for (input, expected) in TEST_CASES {
            buf.clear();
            unsafe { x86_64::avx2_unroll(input, &mut buf) };
            assert_eq!(expected, &buf, "input: `{input}`");
        }
    }
    #[cfg(target_arch = "x86_64")]
    #[test]
    fn test_avx2_unrollx2() {
        if !x86_64::can_run_avx2() {
            return;
        }
        let mut buf = Vec::new();
        for (input, expected) in TEST_CASES {
            buf.clear();
            unsafe { x86_64::avx2_unrollx2(input, &mut buf) };
            assert_eq!(expected, &buf, "input: `{input}`");
        }
    }
}
