// Development-only pinned GDX buffered-stream probe; never linked by Rust.
#include "gmsstrm.hpp"
#include <algorithm>
#include <array>
#include <fstream>
#include <iostream>
#include <stdexcept>
#include <string>

int main(int argc, char **argv) {
    try {
        if (argc != 4) throw std::runtime_error("usage: codec decode|encode INPUT OUTPUT");
        const std::string mode = argv[1];
        if (mode != "encode" && mode != "decode") throw std::runtime_error("invalid mode");
        // Fixed buffer and bounded tiny-fixture total; no input-sized collection.
        gdlib::gmsstrm::TBufferedFileStream input(argv[2], gdlib::gmsstrm::fmOpenRead);
        gdlib::gmsstrm::TBufferedFileStream output(argv[3], gdlib::gmsstrm::fmCreate);
        if (input.GetLastIOResult() || output.GetLastIOResult()) throw std::runtime_error("open failed");
        input.SetCompression(mode == "decode");
        output.SetCompression(mode == "encode");
        if (!input.GetCanCompress() || !output.GetCanCompress()) throw std::runtime_error("codec unavailable");
        std::array<unsigned char,4096> bytes{};
        unsigned total = 0;
        for (;;) {
            const auto n = input.Read(bytes.data(), bytes.size());
            if (input.GetLastIOResult()) throw std::runtime_error("read failed");
            if (!n) break;
            total += n;
            if (total > 131072) throw std::runtime_error("fixture bound exceeded");
            if (output.Write(bytes.data(),n) != n) throw std::runtime_error("short write");
        }
        if (!output.FlushBuffer() || output.GetLastIOResult()) throw std::runtime_error("flush failed");
        std::cout << total << '\n';
    } catch (const std::exception &e) { std::cerr << e.what() << '\n'; return 1; }
}
