An informal benchmark for splitting a string on newlines. While this is a rather boring problem, the ideas can be applied to other algorithms, especially parsing.

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
* Be careful when using collections with vector code - `Vec::push` compiles to a function call, which means your vector registers have to be reloaded. This *really* hurts in more complicated algorithms. Generally, if you're writing a tight SIMD loop, check the ASM to see where the compiler is inserting loads for constants. If it's happening every iteration of the inner loop, something's wrong and it's probably a function call.
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

## 9th gen Intel

### Slice

| algo | single line | 1-20 | 5-20 | 10-30 | 40-50 | all lines |
| :-- | --: | --: | --: | --: | --: | --: |
| std         | 11875 |  331 |  377 |  547 | 1108 |  61 |
| std_reuse   | 12004 |  542 |  628 |  893 | 1944 | 173 |
| sse2        |  9658 | 1941 | 2535 | 2992 | 5487 | 473 |
| sse2_unsafe |  9653 | 2243 | 2778 | 3209 | 5882 | 675 | 
| sse2_unroll | 11129 | 2116 | 2759 | 3452 | 5919 | 767 |
| avx2        | 14857 | 2019 | 2409 | 3177 | 6106 | 323 |
| avx2_unsafe | 14798 | 2794 | 3277 | 3985 | 7453 | 622 |
| avx2_unroll | 18634 | 2725 | 3351 | 4423 | 7273 | 731 |

### Compress

| algo | single line | 1-20 | 5-20 | 10-30 | 40-50 | all lines |
| :-- | --: | --: | --: | --: | --: | --: |
| iter        |  2055 |  878 | 1259 | 1259 |  1621 |  537 |
| sse2        | 11092 | 2522 | 3122 | 4090 |  7636 | 1305 |
| sse2 unroll | 14743 | 2594 | 3279 | 4207 |  7753 | 1543 |
| avx2 unroll | 18709 | 3380 | 4396 | 5359 | 10692 | 1801 |

## CPU w/ AVX512

### Slice

| algo | single line | 1-20 | 5-20 | 10-30 | 40-50 | all lines |
| :-- | --: | --: | --: | --: | --: | --: |
| std         | 22119 |  731 |  826 | 1129 |  2517 |  152 |
| std_reuse   | 21988 |  794 |  900 | 1248 |  2956 |  180 |
| sse2        | 23833 | 3055 | 4099 | 5031 | 10837 |  698 |
| sse2_unsafe | 23658 | 3391 | 4532 | 5475 | 11147 | 1189 |
| sse2_unroll | 29178 | 3708 | 4992 | 6179 | 13575 | 1186 |
| avx2        | 36360 | 4029 | 4915 | 6708 | 13425 |  741 |
| avx2_unsafe | 36895 | 4657 | 5640 | 7561 | 14752 | 1232 |
| avx2_unroll | 50930 | 4662 | 5850 | 8031 | 18306 | 1145 |

### Compress

| algo | single line | 1-20 | 5-20 | 10-30 | 40-50 | all lines |
| :-- | --: | --: | --: | --: | --: | --: |
| iter        |  3874 |  1344 |  1725 |  2218 |  3210 |  1371 |
| sse2        | 37064 |  3674 |  5052 |  6494 | 14724 |  2154 |
| sse2 unroll | 33591 |  3881 |  5440 |  6446 | 15919 |  2339 |
| avx2 unroll | 52322 |  5452 |  6902 |  8675 | 21244 |  3154 |
| avx512      | 50391 | 32130 | 32708 | 37786 | 36360 | 11503 |
