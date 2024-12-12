An informal benchmark for splitting a string on newlines. While this is a rather boring problem, the ideas can be applied to other algorithms, especially parsing.

[skip to benchmark results](#my-results)

## Notes
If you want to write a _really_ fast parser, you almost certainly don't want to split on newlines and then iterate over the lines.
You'd be reading bytes twice. Instead, you'd write your parsing logic as "repeat this until newline".
Of course, keep in mind the actual input sizes and optimize where it's worthwhile.

# Takeaways
* The obvious solution built using standard library functions (labeled `std`) is unreasonably slow.
* When possible, avoid constructing a new collection - take it as a `&mut` parameter instead.
* Instead of storing slices, store the positions of newlines - it's the same information, but significantly less space. More generally, consider the redundancy between elements, and whether you can compress them somehow.
* Even just `sse2`, which is standard on `x86-64`, helps to greatly accelerate basic string operations.
* Bounds checks can significantly pessimize this type of code - be sure to skim the assembly of your inner loops for unexpected branches.
* Be careful when using collections with SIMD code - `Vec::push` compiles to a function call, which means your vector registers have to be reloaded. This *really* hurts in more complicated algorithms. Generally, if you're writing a tight SIMD loop, check the ASM to see where the compiler is inserting loads for constants. If it's happening every iteration of the inner loop, something's wrong and it's probably a function call.
* `vpcompressb` is still the greatest thing since sliced bread. (seen in the AVX512 algo)

The `LineIndex` data structure stores newline positions in a compressed form. 2 bytes per newline, and 8 bytes per 64KB of input. For comparison, a string slice in a 64 bit Rust program takes 16 bytes.
For 1000 lines, that's 16KB vs 2KB - 8x smaller. Constructing the slice on the fly from the compressed data is just a few cycles. Also, an array of small indexes is a lot easier for SIMD code to work with.

Below is a pattern I like for avoiding functions calls for reallocating `Vec`s when in a loop, with relevant lines tagged by a trailing line comment:
```rust
let mut write_i = 0; //
out.lows.reserve(256); // Make sure there's room for 256 elements
unsafe {
    let out_arr = out.lows.spare_capacity_mut().get_unchecked_mut(..256); // Get a &mut [MaybeUninit<T>]. A write-only array, basically.
    while write_i <= (256 - 16) && chunk_i < stop_chunk_i { // (This loop can produce no more than 16 elements per iter)
        let v = _mm_loadu_si128(chunk_64k.as_ptr().add(chunk_i * 16).cast());
        let mut mask = _mm_movemask_epi8(_mm_cmpeq_epi8(v, nl_v)) as u16;
        while mask != 0 {
            let bit_pos = mask.trailing_zeros() as u16;
            out_arr
                .get_unchecked_mut(write_i)
                .write(chunk_i as u16 * 16 + bit_pos); // Replace `Vec::push` with `arr[i].write(e)`
            write_i += 1; //
            mask &= mask - 1;
        }
        chunk_i += 1;
    }
    out.lows.set_len(out.lows.len() + write_i); // Update the length with number of new elements
}
```


# My Results
`single line` -> no newlines in input  
`M-N` -> each line is M to N bytes long  
`all lines` -> every byte is a newline

`std_reuse` -> `std` but with an existing `Vec`  
`*unsafe` -> removed bounds checks  
`*unroll` -> pulled alloc-y calls out of the inner loop  

Throughput in MB/s of input. AVX512 results are on the very last line.

## TL;DR
The `sse2_unrollx4` variants perform very well and are probably the easiest to integrate (runs on any x86-64 CPU).

## 9th gen Intel

### Slice

| algo | single line | 1-20 | 5-20 | 10-30 | 0-40 | 0-80 | 40-120 | all lines |
| :-- | --: | --: | --: | --: | --: | --: | --: | --: |
| std_reuse     | 12384 | 604  | 696  | 961  | 945  | 1518 | 2719 | 209 |
| sse2          | 9634  | 2050 | 2588 | 3025 | 2538 | 3299 | 4596 | 490 |
| sse2_unsafe   | 9964  | 2237 | 2854 | 3396 | 2659 | 3384 | 4626 | 705 |
| sse2_unroll   | 11201 | 2192 | 2889 | 3454 | 2738 | 3701 | 5334 | 780 |
| sse2_unrollx4 | 17145 | 3855 | 4656 | 5987 | 4913 | 6191 | 9010 | 791 |
| avx2          | 14854 | 2051 | 2477 | 3411 | 3033 | 3873 | 5683 | 309 |
| avx2_unsafe   | 14854 | 2786 | 3278 | 4266 | 3615 | 4321 | 6088 | 635 |
| avx2_unroll   | 18567 | 2940 | 3563 | 4622 | 3907 | 5138 | 7794 | 797 |
| avx2_unrollx2 | 19619 | 3500 | 4382 | 5586 | 4903 | 6334 | 9126 | 790 |

### Compress

| algo | single line | 1-20 | 5-20 | 10-30 | 0-40 | 0-80 | 40-120 | all lines |
| :-- | --: | --: | --: | --: | --: | --: | --: | --: |
| iter          | 2075  | 875  | 1034 | 1278 | 1281 | 1563 | 1802 | 621  |
| sse2          | 11448 | 2636 | 3423 | 4255 | 3243 | 4471 | 6562 | 1310 |
| sse2 unroll   | 14906 | 2667 | 3588 | 4506 | 3421 | 4595 | 6850 | 1570 |
| sse2 unrollx4 | 16704 | 4567 | 5500 | 6760 | 5554 | 6599 | 9040 | 1687 |
| avx2 unroll   | 18956 | 3428 | 4319 | 5302 | 4475 | 5672 | 8152 | 1745 |
| avx2 unrollx2 | 19158 | 4508 | 5223 | 6581 | 5430 | 6260 | 8793 | 1799 |

## CPU w/ AVX512

### Slice

| algo | single line | 1-20 | 5-20 | 10-30 | 0-40 | 0-80 | 40-120 | all lines |
| :-- | --: | --: | --: | --: | --: | --: | --: | --: |
| std           | 22031 |  723 |  836 |  1166 | 1172 |  2054 |  3647 |  147 |
| std_reuse     | 21967 | 1064 | 1180 |  1583 | 1587 |  2692 |  4681 |  367 |
| sse2          | 23739 | 3010 | 3961 |  4833 | 3814 |  5393 |  8510 |  821 |
| sse2_unsafe   | 23635 | 3330 | 4447 |  5400 | 4089 |  5511 |  8684 | 1169 |
| sse2_unroll   | 29939 | 3792 | 5151 |  6379 | 4716 |  6826 | 10952 | 1176 |
| sse2_unrollx4 | 40701 | 6792 | 8338 | 11082 | 9100 | 11545 | 16059 | 1410 |
| avx2          | 36927 | 4068 | 5019 |  6880 | 5755 |  7307 | 11653 |  745 |
| avx2_unsafe   | 36845 | 4627 | 5638 |  7476 | 6165 |  7608 | 12037 | 1216 |
| avx2_unroll   | 53034 | 4773 | 6075 |  8258 | 6742 |  8782 | 14128 | 1223 |
| avx2_unrollx2 | 52315 | 7006 | 8571 | 11637 | 9463 | 12345 | 19387 | 1399 |

### Compress

| algo | single line | 1-20 | 5-20 | 10-30 | 0-40 | 0-80 | 40-120 | all lines |
| :-- | --: | --: | --: | --: | --: | --: | --: | --: |
| iter          |  5374 |  1234 |  1374 |  1674 |  1685 |  2098 |  2564 |  1397 |
| sse2          | 35827 |  3720 |  5142 |  6455 |  4695 |  6713 | 11206 |  2083 |
| sse2 unroll   | 33535 |  3889 |  5454 |  6529 |  4852 |  6874 | 10773 |  2307 |
| sse2 unrollx4 | 42100 |  6866 |  8645 | 11524 |  8795 | 12066 | 17754 |  1950 |
| avx2 unroll   | 49541 |  5639 |  7179 |  9124 |  7472 |  9202 | 14216 |  3160 |
| avx2 unrollx2 | 50083 |  7916 |  9988 | 13206 |  9961 | 12613 | 19919 |  2667 |
| avx512        | 53305 | 32729 | 34333 | 38183 | 36737 | 33878 | 19124 | 12447 |
