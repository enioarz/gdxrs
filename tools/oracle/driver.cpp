// Development-only reference driver. Does not form part of the Rust library.
#include "gdx.hpp"
#include <array>
#include <cstdint>
#include <cstring>
#include <fstream>
#include <iomanip>
#include <iostream>
#include <sstream>
#include <stdexcept>
#include <string>

using gdx::TGXFileObj;

static void require(TGXFileObj &g, int ok, const char *operation) {
    if (!ok) {
        char message[GMS_SSSIZE]{};
        int error = g.gdxGetLastError();
        g.gdxErrorStr(error, message);
        throw std::runtime_error(std::string(operation) + ": " + std::to_string(error) + " " + message);
    }
}

static std::string hex(const char *s) {
    std::ostringstream out;
    out << std::hex << std::setfill('0');
    for (const unsigned char *p = reinterpret_cast<const unsigned char *>(s); *p; ++p)
        out << std::setw(2) << static_cast<unsigned>(*p);
    return out.str();
}

static std::string value(TGXFileObj &g, double v) {
    int special{};
    if (g.gdxMapValue(v, special)) {
        static const char *names[] = {"undef", "na", "pinf", "minf", "eps"};
        if (special < 0 || special > 4) throw std::runtime_error("unknown mapped special");
        return std::string("s:") + names[special];
    }
    int acronym = g.gdxAcronymIndex(v);
    if (acronym) return "a:" + std::to_string(acronym);
    std::uint64_t bits;
    static_assert(sizeof(bits) == sizeof(v));
    std::memcpy(&bits, &v, sizeof(bits));
    std::ostringstream out;
    out << "f:" << std::hex << std::setw(16) << std::setfill('0') << bits;
    return out.str();
}

static void smoke(const char *path, int compression) {
    TGXFileObj g;
    int error{};
    require(g, g.gdxOpenWriteEx(path, "gdxrs-oracle", compression, error), "open write");
    g.gdxAutoConvert(0);
    require(g, g.gdxUELRegisterRawStart(), "register start");
    for (auto label : {"z", "a", "b"}) require(g, g.gdxUELRegisterRaw(label), "register label");
    require(g, g.gdxUELRegisterDone(), "register done");
    int text{};
    require(g, g.gdxAddSetText("set element", text), "add text");
    int key[20]{};
    double vals[5]{};
    require(g, g.gdxDataWriteRawStart("i", "set", 1, GMS_DT_SET, 0), "set start");
    key[0] = 1; vals[0] = text;
    require(g, g.gdxDataWriteRaw(key, vals), "set row");
    key[0] = 2; vals[0] = 0;
    require(g, g.gdxDataWriteRaw(key, vals), "set row");
    require(g, g.gdxDataWriteDone(), "set done");
    require(g, g.gdxSymbolAddComment(1, "smoke comment"), "comment");
    require(g, g.gdxAcronymAdd("ac", "acronym", 7), "acronym");
    require(g, g.gdxDataWriteRawStart("p", "parameter", 2, GMS_DT_PAR, 0), "parameter start");
    const char *domains[] = {"i", "*"};
    require(g, g.gdxSymbolSetDomain(domains), "strict domain");
    key[0] = 1; key[1] = 2; vals[0] = 2.5;
    require(g, g.gdxDataWriteRaw(key, vals), "parameter row");
    key[1] = 3; vals[0] = GMS_SV_NA;
    require(g, g.gdxDataWriteRaw(key, vals), "parameter row");
    key[0] = 2; key[1] = 1; vals[0] = g.gdxAcronymValue(7);
    require(g, g.gdxDataWriteRaw(key, vals), "parameter row");
    require(g, g.gdxDataWriteDone(), "parameter done");
    require(g, g.gdxDataWriteRawStart("x", "variable", 1, GMS_DT_VAR, GMS_VARTYPE_FREE), "variable start");
    key[0] = 3;
    vals[0] = 1.25; vals[1] = -2.5; vals[2] = GMS_SV_MINF; vals[3] = GMS_SV_PINF; vals[4] = 4;
    require(g, g.gdxDataWriteRaw(key, vals), "variable row");
    require(g, g.gdxDataWriteDone(), "variable done");
    if (g.gdxDataErrorCount()) throw std::runtime_error("unexpected generator data errors");
    if (g.gdxClose()) throw std::runtime_error("close write failed");
}

#include "fixture_generator.hpp"

static void dump(const char *path, std::ostream &out) {
    TGXFileObj g;
    int error{};
    require(g, g.gdxOpenRead(path, error), "open read");
    int symbols{}, uels{}, version{}, compression{};
    require(g, g.gdxSystemInfo(symbols, uels), "system info");
    require(g, g.gdxFileInfo(version, compression), "file info");
    char file_version[GMS_SSSIZE]{}, producer[GMS_SSSIZE]{};
    require(g, g.gdxFileVersion(file_version, producer), "file version");
    out << "GDXRS-ORACLE\t1\nF\t" << version << '\t' << compression << '\t'
        << symbols << '\t' << uels << '\t' << hex(producer) << '\t' << hex(file_version) << '\n';
    for (int n = 1; n <= uels; ++n) {
        char label[GMS_SSSIZE]{}; int mapped{};
        require(g, g.gdxUMUelGet(n, label, mapped), "raw UEL");
        out << "U\t" << n << '\t' << hex(label) << '\n';
    }
    for (int n = 0;; ++n) {
        char text[GMS_SSSIZE]{}; int node{};
        if (!g.gdxGetElemText(n, text, node)) break;
        out << "T\t" << n << '\t' << node << '\t' << hex(text) << '\n';
    }
    for (int n = 1; n <= g.gdxAcronymCount(); ++n) {
        char name[GMS_SSSIZE]{}, text[GMS_SSSIZE]{}; int index{};
        require(g, g.gdxAcronymGetInfo(n, name, text, index), "acronym info");
        out << "A\t" << n << '\t' << index << '\t' << hex(name) << '\t' << hex(text) << '\n';
    }
    for (int n = 0; n <= symbols; ++n) {
        char name[GMS_SSSIZE]{}, text[GMS_SSSIZE]{};
        int dim{}, type{}, count{}, user{};
        require(g, g.gdxSymbolInfo(n, name, dim, type), "symbol info");
        require(g, g.gdxSymbolInfoX(n, count, user, text), "symbol info extended");
        out << "S\t" << n << '\t' << hex(name) << '\t' << dim << '\t' << type
            << '\t' << count << '\t' << user << '\t' << hex(text) << '\n';
        if (n > 0) {
            int strict[20]{};
            require(g, g.gdxSymbolGetDomain(n, strict), "domain symbols");
            std::array<std::array<char, GMS_SSSIZE>, 20> labels{};
            char *ptrs[20]{};
            for (int d = 0; d < 20; ++d) ptrs[d] = labels[d].data();
            int mode = g.gdxSymbolGetDomainX(n, ptrs);
            require(g, mode, "domain strings");
            out << "D\t" << n << '\t' << mode;
            for (int d = 0; d < dim; ++d) out << '\t' << strict[d] << ':' << hex(ptrs[d]);
            out << '\n';
            for (int c = 1;; ++c) {
                char comment[GMS_SSSIZE]{};
                if (!g.gdxSymbolGetComment(n, c, comment)) break;
                out << "C\t" << n << '\t' << c << '\t' << hex(comment) << '\n';
            }
        }
        int expected{};
        require(g, g.gdxDataReadRawStart(n, expected), "read start");
        std::uint64_t seen{};
        int keys[20]{}, first{}; double vals[5]{};
        while (g.gdxDataReadRaw(keys, vals, first)) {
            out << "R\t" << n << '\t';
            for (int d = 0; d < dim; ++d) out << (d ? "," : "") << keys[d];
            out << '\t';
            int fields = (type == GMS_DT_VAR || type == GMS_DT_EQU) ? 5 : 1;
            for (int f = 0; f < fields; ++f) out << (f ? "," : "") << value(g, vals[f]);
            out << '\n'; ++seen;
        }
        require(g, g.gdxDataReadDone(), "read done");
        int last = g.gdxGetLastError();
        if (last) throw std::runtime_error("read error " + std::to_string(last));
        if (expected < 0 || seen != static_cast<std::uint64_t>(expected))
            throw std::runtime_error("record count differs from read-start count");
        out << "E\t" << n << '\t' << seen << '\n';
    }
    if (g.gdxClose()) throw std::runtime_error("close read failed");
    out.flush();
    if (!out) throw std::runtime_error("output failed");
}

// Development-only mapped-read qualification. Mapping recipes are bounded and
// explicit; caller maps are session state, not bytes read from the GDX file.
static void mapped_dump(const char *path, const char *mapping, int symbol) {
    TGXFileObj g; int error{};
    require(g, g.gdxOpenRead(path,error),"mapped open");
    std::ifstream in(mapping); if (!in) throw std::runtime_error("missing map recipe");
    require(g,g.gdxUELRegisterMapStart(),"mapped register start");
    std::string line; int assignments{};
    while (std::getline(in,line)) {
        if (++assignments>10000 || line.size()>600) throw std::runtime_error("map recipe bound exceeded");
        auto fields=split(line);if(fields.size()!=2)throw std::runtime_error("invalid map recipe");
        int id=std::stoi(fields[0]);if(id<0 || id>100000)throw std::runtime_error("native dense map recipe bound exceeded");
        require(g,g.gdxUELRegisterMap(id,unhex(fields[1]).c_str()),"mapped registration");
    }
    require(g,g.gdxUELRegisterDone(),"mapped register done");
    char name[GMS_SSSIZE]{};int dim{},type{},expected{};
    require(g,g.gdxSymbolInfo(symbol,name,dim,type),"mapped symbol");
    require(g,g.gdxDataReadMapStart(symbol,expected),"mapped read start");
    std::cout<<"GDXRS-MAPPED\t1\n";
    int keys[20]{},first{};double values[5]{};int seen{};
    while(g.gdxDataReadMap(seen+1,keys,values,first)) {
        std::cout<<"R\t";for(int d=0;d<dim;++d)std::cout<<(d?",":"")<<keys[d];
        std::cout<<'\t';int fields=(type==GMS_DT_VAR || type==GMS_DT_EQU)?5:1;
        for(int f=0;f<fields;++f)std::cout<<(f?",":"")<<value(g,values[f]);
        std::cout<<'\t';
        for(int d=0;d<dim;++d) {
            char label[GMS_SSSIZE]{};int raw{},mapped{};
            require(g,g.gdxGetUEL(keys[d],label),"inverse mapped label");
            require(g,g.gdxUMFindUEL(label,raw,mapped),"original mapped UEL");
            if(mapped!=keys[d])throw std::runtime_error("non-bijective qualification map");
            std::cout<<(d?",":"")<<raw;
        }
        std::cout<<'\n';++seen;
    }
    std::cout<<"E\t"<<seen<<'\t'<<g.gdxDataErrorCount()<<'\n';
    require(g,g.gdxDataReadDone(),"mapped read done");
    if(expected!=seen)throw std::runtime_error("mapped count mismatch");
    if(g.gdxClose())throw std::runtime_error("mapped close failed");
}
int main(int argc, char **argv) {
    try {
        if (argc == 4 && std::string(argv[1]) == "smoke") {
            std::string level = argv[3];
            if (level != "0" && level != "1") throw std::runtime_error("compression must be 0 or 1");
            smoke(argv[2], level == "1");
        } else if (argc == 5 && std::string(argv[1]) == "recipe") {
            std::string level = argv[4];
            if (level != "0" && level != "1") throw std::runtime_error("compression must be 0 or 1");
            recipe(argv[2], argv[3], level == "1");
        } else if (argc == 5 && std::string(argv[1]) == "mapped-dump") {
            mapped_dump(argv[2],argv[3],std::stoi(argv[4]));
        } else if ((argc == 3 || argc == 4) && std::string(argv[1]) == "dump") {
            if (argc == 3) dump(argv[2], std::cout);
            else {
                std::ofstream out(argv[3], std::ios::binary | std::ios::trunc);
                if (!out) throw std::runtime_error("cannot open output");
                dump(argv[2], out);
            }
        } else throw std::runtime_error("usage: gdxrs-oracle smoke FILE {0|1} | dump FILE [OUTPUT]");
        return 0;
    } catch (const std::exception &e) {
        std::cerr << "oracle: " << e.what() << '\n';
        return 1;
    }
}
