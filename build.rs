extern crate cc;

fn main() {
    let mut base = cc::Build::new();

    // Only add MSVC optimization flags when the target is actually MSVC
    // (GitHub Actions Windows uses GNU/MinGW, so skip them there)
    if std::env::var("CARGO_CFG_TARGET_ENV").unwrap() == "msvc" {
        base.flag("/O2")
            .flag("/Oi")
            .flag("/Ot")
            .flag("/Oy")
            .flag("/GT")
            .flag("/GL");
    } else {
        base.flag("-std=c99")
            .flag("-mtune=native");
    }

    // Base shabal (always)
    let mut config = base.clone();
    config.file("src/c/sph_shabal.c")
          .file("src/c/common.c")
          .compile("shabal");

    // SIMD variants only on x86_64 (skip on arm64 etc.)
    if std::env::var("CARGO_CFG_TARGET_ARCH").unwrap() == "x86_64" {
        // SSE2
        let mut config = base.clone();
        if std::env::var("CARGO_CFG_TARGET_ENV").unwrap() != "msvc" {
            config.flag("-msse2");
        }
        config.file("src/c/mshabal_128_sse2.c")
              .file("src/c/noncegen_128_sse2.c")
              .compile("shabal_sse2");

        // AVX2
        let mut config = base.clone();
        if std::env::var("CARGO_CFG_TARGET_ENV").unwrap() == "msvc" {
            config.flag("/arch:AVX2");
        } else {
            config.flag("-mavx2");
        }
        config.file("src/c/mshabal_256_avx2.c")
              .file("src/c/noncegen_256_avx2.c")
              .compile("shabal_avx2");

        // AVX
        let mut config = base.clone();
        if std::env::var("CARGO_CFG_TARGET_ENV").unwrap() == "msvc" {
            config.flag("/arch:AVX");
        } else {
            config.flag("-mavx");
        }
        config.file("src/c/mshabal_128_avx.c")
              .file("src/c/noncegen_128_avx.c")
              .compile("shabal_avx");

        // AVX512
        let mut config = base.clone();
        if std::env::var("CARGO_CFG_TARGET_ENV").unwrap() == "msvc" {
            config.flag("/arch:AVX512");
        } else {
            config.flag("-mavx512f");
        }
        config.file("src/c/mshabal_512_avx512f.c")
              .file("src/c/noncegen_512_avx512f.c")
              .compile("shabal_avx512");
    }
}