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

## Slice

| algo | single line | 1-20 | 5-20 | 10-30 | 0-40 | 0-80 | 40-120 | 0-0 |
| :-- | --: | --: | --: | --: | --: | --: | --: | --: |
| std | 11889 | 350 | 402 | 572 | 571 | 1007 | 1805 | 64 |
| std_reuse | 11763 | 566 | 655 | 896 | 867 | 1447 | 2515 | 177 |
| sse2 | 9821 | 1648 | 2061 | 2531 | 1984 | 2791 | 4095 | 331 |
| sse2_unsafe | 9828 | 2049 | 2635 | 3136 | 2460 | 3162 | 4478 | 603 |
| sse2_unroll | 11183 | 2188 | 2891 | 3426 | 2712 | 3585 | 5252 | 780 |
| sse2_unrollx4 | 16455 | 3854 | 4505 | 5822 | 4731 | 5970 | 8741 | 786 |
| avx2 | 15247 | 2254 | 2652 | 3669 | 3082 | 3874 | 5674 | 379 |
| avx2_unsafe | 14904 | 3062 | 3552 | 4400 | 3754 | 4451 | 6181 | 696 |
| avx2_unroll | 18104 | 2767 | 3070 | 3984 | 3497 | 4475 | 6675 | 783 |
| avx2_unrollx2 | 19218 | 3235 | 3606 | 5067 | 4364 | 5423 | 7901 | 786 |

## Compressed format

| algo | single line | 1-20 | 5-20 | 10-30 | 0-40 | 0-80 | 40-120 | 0-0 |
| :-- | --: | --: | --: | --: | --: | --: | --: | --: |
| iter | 2864 | 900 | 1027 | 1374 | 1369 | 1862 | 2157 | 599 |
| sse2 | 11324 | 2317 | 3034 | 3624 | 2718 | 3808 | 5727 | 1320 |
| sse2 unroll | 12966 | 2417 | 3117 | 3712 | 2762 | 3788 | 5745 | 1562 |
| sse2 unrollx4 | 16713 | 4547 | 5335 | 6740 | 5542 | 6441 | 9116 | 1675 |
| sse4 intrlv | 16676 | 5676 | 6683 | 8134 | 6726 | 8126 | 12062 | 1942 |
| avx2 unroll | 18656 | 3508 | 4139 | 5186 | 4350 | 5582 | 7940 | 1768 |
| avx2 unrollx2 | 19270 | 4462 | 5096 | 6544 | 5303 | 6159 | 8751 | 1798 |
| avx2 intrlv | 18658 | 6175 | 7136 | 8389 | 7203 | 8020 | 12004 | 1550 |

## CPU w/ AVX512

## Slice

| algo | single line | 1-20 | 5-20 | 10-30 | 0-40 | 0-80 | 40-120 | 0-0 |
| :-- | --: | --: | --: | --: | --: | --: | --: | --: |
| std | 22059 | 725 | 852 | 1168 | 1174 | 2057 | 3633 | 155 |
| std_reuse | 22050 | 1008 | 1140 | 1528 | 1543 | 2633 | 4725 | 381 |
| sse2 | 23824 | 3153 | 4238 | 5165 | 4011 | 5738 | 9052 | 844 |
| sse2_unsafe | 23721 | 3429 | 4610 | 5544 | 4197 | 5798 | 9188 | 1217 |
| sse2_unroll | 29979 | 3817 | 4289 | 6381 | 4805 | 6899 | 10948 | 1201 |
| sse2_unrollx4 | 36598 | 6837 | 8399 | 11179 | 9212 | 11720 | 16121 | 1421 |
| avx2 | 36324 | 4334 | 5201 | 7221 | 6053 | 7444 | 12014 | 831 |
| avx2_unsafe | 36254 | 5012 | 6137 | 7717 | 6451 | 7848 | 12214 | 1306 |
| avx2_unroll | 49963 | 5240 | 6376 | 8228 | 6468 | 8727 | 13561 | 1521 |
| avx2_unrollx2 | 50066 | 7204 | 8667 | 11369 | 9221 | 11520 | 18007 | 1495 |

## Compressed format

| algo | single line | 1-20 | 5-20 | 10-30 | 0-40 | 0-80 | 40-120 | 0-0 |
| :-- | --: | --: | --: | --: | --: | --: | --: | --: |
| iter | 5518 | 1589 | 1870 | 2432 | 2430 | 3206 | 3837 | 2093 |
| sse2 | 36697 | 1759 | 2368 | 2951 | 2017 | 2660 | 4631 | 894 |
| sse2 unroll | 33233 | 3892 | 5456 | 6432 | 4840 | 6804 | 11112 | 2371 |
| sse2 unrollx4 | 40384 | 6823 | 8516 | 11850 | 9136 | 11656 | 16702 | 1948 |
| sse4 intrlv | 38550 | 10092 | 11720 | 14652 | 12327 | 14931 | 24193 | 3225 |
| avx2 unroll | 50107 | 5350 | 7262 | 9200 | 7700 | 9130 | 15516 | 3202 |
| avx2 unrollx2 | 49667 | 7855 | 10014 | 13243 | 9988 | 12992 | 20626 | 3272 |
| avx2 intrlv | 49085 | 11212 | 13327 | 16337 | 13326 | 16470 | 29655 | 3882 |
| avx512 | 42968 | 28785 | 30149 | 38223 | 32001 | 35850 | 34813 | 12260 |

