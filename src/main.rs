// only enable avx512 for x86-64 nightly builds
#![cfg_attr(
    all(feature = "nightly", target_arch = "x86_64"),
    feature(avx512_target_feature)
)]
#![cfg_attr(
    all(feature = "nightly", target_arch = "x86_64"),
    feature(stdarch_x86_avx512)
)]

fn std(input: &str) -> Vec<&str> {
    input.lines().collect()
}

fn std_reuse<'input, 'buf>(input: &'input str, out: &'buf mut Vec<&'input str>) {
    for line in input.lines() {
        out.push(line);
    }
}

#[cfg(target_arch = "x86_64")]
mod x86_64 {
    use std::arch::x86_64::*;

    pub fn sse2<'input, 'buf>(input: &'input str, out: &'buf mut Vec<&'input str>) {
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

    fn tail<'input, 'buf>(
        mut line_start: usize,
        chunk_size: usize,
        input: &'input str,
        out: &'buf mut Vec<&'input str>,
    ) {
        // handle last bytes
        for i in (input.len() & !(chunk_size - 1))..input.len() {
            if input.as_bytes()[i] != b'\n' {
                continue;
            }
            debug_assert!(line_start <= i);
            out.push(unsafe { input.get_unchecked(line_start..i) });
            line_start = i + 1;
        }
        // handle last line. omit if empty
        if line_start != input.len() {
            debug_assert!(line_start <= input.len());
            out.push(unsafe { input.get_unchecked(line_start..) });
        }
    }

    pub fn sse2_unsafe<'input, 'buf>(input: &'input str, out: &'buf mut Vec<&'input str>) {
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
                    debug_assert!(line_start <= line_end);
                    out.push(input.get_unchecked(line_start..line_end));
                    line_start = line_end + 1;
                    mask &= mask - 1;
                }
            }
        }
        tail(line_start, 16, input, out);
    }

    pub fn sse2_unroll<'input, 'buf>(input: &'input str, out: &'buf mut Vec<&'input str>) {
        // Key idea is to pull the allocation out of the innermost loop

        let mut line_start = 0;
        unsafe {
            let nl_v = _mm_loadu_si128([b'\n'; 16].as_ptr().cast());
            let mut chunk_i = 0;
            let stop_chunk_i = input.len() / 16;
            while chunk_i < stop_chunk_i {
                let mut write_i = 0;
                out.reserve(64);
                let out_arr = out.spare_capacity_mut().get_unchecked_mut(..64);
                while write_i < (64 - 16) && chunk_i < stop_chunk_i {
                    let v = _mm_loadu_si128(input.as_ptr().byte_add(chunk_i * 16).cast());
                    let mut mask = _mm_movemask_epi8(_mm_cmpeq_epi8(v, nl_v)) as u16;
                    while mask != 0 {
                        let bit_pos = mask.trailing_zeros() as usize;
                        let line_end = chunk_i * 16 + bit_pos;
                        debug_assert!(line_start <= line_end);
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

    pub fn can_run_avx2() -> bool {
        is_x86_feature_detected!("avx2")
    }

    #[target_feature(enable = "avx2")]
    pub unsafe fn avx2<'input, 'buf>(input: &'input str, out: &'buf mut Vec<&'input str>) {
        // scan 32-byte chunks, then handle tail
        let mut line_start = 0;
        let nl_v = _mm256_loadu_si256([b'\n'; 32].as_ptr().cast());
        for (chunk_i, chunk) in input.as_bytes().chunks_exact(32).enumerate() {
            let v = _mm256_loadu_si256(chunk.as_ptr().cast());
            let mut mask = _mm256_movemask_epi8(_mm256_cmpeq_epi8(v, nl_v)) as u32;
            while mask != 0 {
                let bit_pos = mask.trailing_zeros() as usize;
                let line_end = chunk_i * 32 + bit_pos;
                debug_assert!(line_start <= line_end);
                out.push(&input[line_start..line_end]);
                line_start = line_end + 1;
                mask &= mask - 1;
            }
        }
        tail(line_start, 32, input, out);
    }

    #[target_feature(enable = "avx2")]
    pub unsafe fn avx2_unsafe<'input, 'buf>(input: &'input str, out: &'buf mut Vec<&'input str>) {
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

    #[target_feature(enable = "avx2")]
    pub unsafe fn avx2_unroll<'input, 'buf>(input: &'input str, out: &'buf mut Vec<&'input str>) {
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
            out.reserve(64);
            let out_arr = out.spare_capacity_mut().get_unchecked_mut(..64);
            // at most 32 items will be added per chunk
            while write_i <= (64 - 32) && chunk_i < stop_chunk_i {
                let v = _mm256_loadu_si256(input.as_ptr().byte_add(chunk_i * 32).cast());
                let mut mask = _mm256_movemask_epi8(_mm256_cmpeq_epi8(v, nl_v)) as u32;
                while mask != 0 {
                    let bit_pos = mask.trailing_zeros() as usize;
                    let line_end = chunk_i * 32 + bit_pos;
                    debug_assert!(line_start <= line_end);
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

    #[cfg(feature = "nightly")]
    pub fn can_run_avx512_compress() -> bool {
        is_x86_feature_detected!("avx512f")
            && is_x86_feature_detected!("avx512bw")
            && is_x86_feature_detected!("avx512vbmi")
            && is_x86_feature_detected!("avx512vbmi2")
    }

    #[cfg(feature = "nightly")]
    #[target_feature(enable = "avx512f,avx512bw,avx512vbmi2")]
    pub unsafe fn avx512_compress<'input, 'buf>(
        input: &'input str,
        out: &'buf mut Vec<&'input str>,
    ) {
        // TODO
        // get chunk
        // make newline bitmask
        // compute offsets, carrying prev line start
        // compress and store
        todo!("implement avx512")
    }
}

fn reset_vector<'a, 'b, T: ?Sized>(mut vec: Vec<&'a T>) -> Vec<&'b T> {
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
    // TODO: better heuristic for length cap
    (0..vec.len().min(64 * 1024 * 1024) * 2 / (N + M))
        .collect::<HashSet<usize>>()
        .iter()
        .copied()
        .map(|i| M + (i % (N - M + 1)))
        .for_each(|i| {
            idx += i;
            vec[idx] = b'\n';
        });
    vec.len().min(64 * 1024 * 1024)
}

type SplitFn = for<'a, 'b> fn(&'a str, &'b mut Vec<&'a str>);
type FeatCheckFn = fn() -> bool;

fn main() {
    use std::hint::black_box;
    use std::time::Instant;

    let benchmark_stages: &[(&str, fn(&mut Vec<u8>) -> usize)] = &[
        ("single line", |vec| vec.len()),
        ("1-20 byte lines", prep_vec_range::<1, 20>),
        ("5-20 byte lines", prep_vec_range::<5, 20>),
        ("10-30 byte lines", prep_vec_range::<10, 30>),
        ("40-50 byte lines", prep_vec_range::<40, 50>),
        ("all lines", |vec| {
            vec.fill(b'\n');
            // You might OOM w/ 1 billion
            vec.len().min(64 * 1024 * 1024)
        }),
    ];
    let bench_cases: &[(&str, SplitFn)] = &[
        ("std_reuse", std_reuse),
        #[cfg(target_arch = "x86_64")]
        ("sse2", x86_64::sse2),
        #[cfg(target_arch = "x86_64")]
        ("sse2_unsafe", x86_64::sse2_unsafe),
        #[cfg(target_arch = "x86_64")]
        ("sse2_unroll", x86_64::sse2_unroll),
    ];
    let opt_bench_cases: &[(&str, FeatCheckFn, SplitFn)] = &[
        #[cfg(target_arch = "x86_64")]
        ("avx2", x86_64::can_run_avx2, |a, b| unsafe {
            x86_64::avx2(a, b)
        }),
        #[cfg(target_arch = "x86_64")]
        ("avx2_unsafe", x86_64::can_run_avx2, |a, b| unsafe {
            x86_64::avx2_unsafe(a, b)
        }),
        #[cfg(target_arch = "x86_64")]
        ("avx2_unroll", x86_64::can_run_avx2, |a, b| unsafe {
            x86_64::avx2_unroll(a, b)
        }),
        /*
        #[cfg(all(feature = "nightly", target_arch = "x86_64"))]
        (
        "avx512_compress",
        x86_64::can_run_avx512_compress,
        |a, b| unsafe { x86_64::avx512_compress(a, b) },
        )*/
    ];

    let mut b = vec![b'a'; 1024 * 1024 * 1024];

    // pre-fill the vec (beyond just reserving) so that the first fn doesn't pay for all the page
    // misses (some OSs may give CoW zero pages for `Vec::with_capacity(...)` )
    let mut pool_out_buf = black_box(vec![""; 64 * 1024 * 1024]);

    for (stage_label, prep_fn) in benchmark_stages {
        println!("\n\tstarting {stage_label}");
        let len = prep_fn(&mut b);
        let input = std::str::from_utf8(&b[..len]).unwrap();
        let mut out_buf = pool_out_buf;

        {
            let start = Instant::now();
            black_box(std(input));
            let duration = start.elapsed().as_secs_f64();
            let thrpt = len as f64 / duration / 1_000_000.;
            println!("{fn_label:<13}: {thrpt:>8.0}", fn_label = "std");
        }
        for (fn_label, fnc) in bench_cases {
            out_buf.clear();
            let start = Instant::now();
            fnc(input, &mut out_buf);
            let duration = start.elapsed().as_secs_f64();
            black_box(&mut out_buf);
            let thrpt = len as f64 / duration / 1_000_000.;
            println!("{fn_label:<13}: {thrpt:>8.0}");
        }
        for (fn_label, feat_checker, fnc) in opt_bench_cases {
            if !feat_checker() {
                println!("skipping {fn_label} because of missing CPU features");
            }
            out_buf.clear();
            let start = Instant::now();
            fnc(input, &mut out_buf);
            let duration = start.elapsed().as_secs_f64();
            black_box(&mut out_buf);
            let thrpt = len as f64 / duration / 1_000_000.;
            println!("{fn_label:<13}: {thrpt:>8.0}");
        }
        // run first test case again to show that it's not sensitive to order (e.g. cache)
        {
            let start = Instant::now();
            black_box(std(input));
            let duration = start.elapsed().as_secs_f64();
            let thrpt = len as f64 / duration / 1_000_000.;
            println!("{fn_label:<13}: {thrpt:>8.0}", fn_label = "std");
        }

        pool_out_buf = reset_vector(out_buf);
    }
}

#[cfg(test)]
mod tests {
    use crate::*;

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

    #[cfg(all(feature = "nightly", target_arch = "x86_64"))]
    #[test]
    fn test_avx512_compress() {
        if !x86_64::can_run_avx512_compress() {
            return;
        }
        let mut buf = Vec::new();
        for (input, expected) in TEST_CASES {
            buf.clear();
            unsafe { x86_64::avx512_compress(input, &mut buf) };
            assert_eq!(expected, &buf, "input: `{input}`");
        }
    }
}
