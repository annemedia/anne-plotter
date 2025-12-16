extern crate cc;

use std::env;

fn main() {
    let target_arch = env::var("TARGET").unwrap();

    let is_x86_64 = target_arch.contains("x86_64");

    let mut shared_config = cc::Build::new();

    #[cfg(target_env = "msvc")]
    shared_config
        .flag("/O2")
        .flag("/Oi")
        .flag("/Ot")
        .flag("/Oy")
        .flag("/GT")
        .flag("/GL");

    #[cfg(not(target_env = "msvc"))]
    shared_config.flag("-std=c99").flag("-mtune=native");

    let mut config = shared_config.clone();

    config
        .file("src/c/sph_shabal.c")
        .file("src/c/common.c")
        .compile("shabal");

    // SSE2 variant — only on x86_64
    if is_x86_64 {
        let mut config = shared_config.clone();

        #[cfg(not(target_env = "msvc"))]
        config.flag("-msse2");

        config
            .file("src/c/mshabal_128_sse2.c")
            .file("src/c/noncegen_128_sse2.c")
            .compile("shabal_sse2");
    }

    // AVX2 variant — only on x86_64
    if is_x86_64 {
        let mut config = shared_config.clone();

        if cfg!(target_env = "msvc") {
            config.flag("/arch:AVX2");
        } else {
            config.flag("-mavx2");
        }

        config
            .file("src/c/mshabal_256_avx2.c")
            .file("src/c/noncegen_256_avx2.c")
            .compile("shabal_avx2");
    }

    // AVX variant — only on x86_64
    if is_x86_64 {
        let mut config = shared_config.clone();

        if cfg!(target_env = "msvc") {
            config.flag("/arch:AVX");
        } else {
            config.flag("-mavx");
        }

        config
            .file("src/c/mshabal_128_avx.c")
            .file("src/c/noncegen_128_avx.c")
            .compile("shabal_avx");
    }

    // AVX512 variant — only on x86_64
    if is_x86_64 {
        let mut config = shared_config.clone();

        if cfg!(target_env = "msvc") {
            config.flag("/arch:AVX512");
        } else {
            config.flag("-mavx512f");
        }

        config
            .file("src/c/mshabal_512_avx512f.c")
            .file("src/c/noncegen_512_avx512f.c")
            .compile("shabal_avx512");
    }
}