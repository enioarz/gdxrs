// Development baseline only. The production Rust crate never links this file.
#include "gdx.hpp"
#include <chrono>
#include <cstdint>
#include <cstring>
#include <ctime>
#include <iomanip>
#include <iostream>
#include <sstream>
#include <stdexcept>
#include <string>
#if defined(__linux__) || defined(__APPLE__)
#include <sys/resource.h>
#endif

using gdx::TGXFileObj;
using Clock = std::chrono::steady_clock;

static double seconds(Clock::time_point start) {
    return std::chrono::duration<double>(Clock::now()-start).count();
}
static void check(TGXFileObj &g, int result, const char *what) {
    if (!result) {
        char text[GMS_SSSIZE]{};
        auto code = g.gdxGetLastError(); g.gdxErrorStr(code,text);
        throw std::runtime_error(std::string(what)+": "+std::to_string(code)+" "+text);
    }
}
struct Workload {
    std::string name;
    int compression, rows, labels, symbols, fields, dimension, type, user;
};
static Workload workload(const std::string &name) {
    if (name=="parameter-uncompressed") return {name,0,1000000,1000,1,1,2,GMS_DT_PAR,0};
    if (name=="parameter-compressed") return {name,1,1000000,1000,1,1,2,GMS_DT_PAR,0};
    if (name=="variable") return {name,0,100000,1000,1,5,2,GMS_DT_VAR,GMS_VARTYPE_FREE};
    if (name=="metadata") return {name,0,2000,100000,2000,1,0,GMS_DT_PAR,0};
    throw std::runtime_error("unknown workload");
}
static std::string label(const Workload &w,int n) {
    std::string result="u"+std::to_string(n);
    if (w.name=="metadata") { result += '_'; result.resize(63,'x'); }
    return result;
}
static std::string symbol_name(const Workload &w,int n) {
    return w.symbols==1 ? "data" : "s"+std::to_string(n);
}
static std::uint64_t update(std::uint64_t digest,std::uint64_t value,int bytes) {
    for (int i=0;i<bytes;++i) {
        digest ^= value & 255; digest *= UINT64_C(1099511628211); value >>= 8;
    }
    return digest;
}
static std::uint64_t hash_row(std::uint64_t digest,const int *keys,const double *values,const Workload &w) {
    for (int i=0;i<w.dimension;++i) digest=update(digest,static_cast<std::uint32_t>(keys[i]),4);
    for (int i=0;i<w.fields;++i) {
        std::uint64_t bits;std::memcpy(&bits,&values[i],8);digest=update(digest,bits,8);
    }
    return digest;
}
static std::uint64_t rss() {
#if defined(__linux__) || defined(__APPLE__)
    rusage use{};
    if (getrusage(RUSAGE_SELF,&use)) throw std::runtime_error("getrusage failed");
#if defined(__linux__)
    return static_cast<std::uint64_t>(use.ru_maxrss)*1024;
#else
    return static_cast<std::uint64_t>(use.ru_maxrss);
#endif
#else
    return 0;
#endif
}
static void execute(const std::string &mode,const char *path,const Workload &w) {
    auto wall=Clock::now();auto cpu=std::clock();
    TGXFileObj g;int error{};std::uint64_t count=0,digest=UINT64_C(14695981039346656037);
    double open_time=0,metadata_time=0,scan_time=0,registration_time=0,encode_time=0,finish_time=0;
    double scan_cpu=0;
    if (mode=="write") {
        auto start=Clock::now();check(g,g.gdxOpenWriteEx(path,"gdxrs-benchmark",w.compression,error),"write open");
        g.gdxAutoConvert(0);open_time=seconds(start);
        start=Clock::now();check(g,g.gdxUELRegisterRawStart(),"register start");
        for (int i=1;i<=w.labels;++i) check(g,g.gdxUELRegisterRaw(label(w,i).c_str()),"register label");
        check(g,g.gdxUELRegisterDone(),"register done");registration_time=seconds(start);
        start=Clock::now();
        for (int symbol=1;symbol<=w.symbols;++symbol) {
            check(g,g.gdxDataWriteRawStart(symbol_name(w,symbol).c_str(),"benchmark",w.dimension,w.type,w.user),"symbol start");
            int iterations=w.symbols==1 ? w.rows : 1;
            for (int row=0;row<iterations;++row) {
                int ordinal=w.symbols==1 ? row : symbol-1;
                int keys[20]{};double values[5]{};
                if (w.dimension) { keys[0]=ordinal/1000+1;keys[1]=ordinal%1000+1; }
                values[0]=ordinal+0.25;
                if (w.fields==5) { values[1]=-ordinal-0.5;values[2]=-100.25;values[3]=100.75;values[4]=1; }
                check(g,g.gdxDataWriteRaw(keys,values),"write row");
                ++count; // Expected digest is computed separately by the Python input recipe.
            }
            check(g,g.gdxDataWriteDone(),"symbol done");
            if (g.gdxDataErrorCount()) throw std::runtime_error("benchmark generation data errors");
        }
        encode_time=seconds(start);start=Clock::now();
        if (g.gdxClose()) throw std::runtime_error("write close");
        finish_time=seconds(start);
    } else if (mode=="scan") {
        auto start=Clock::now();check(g,g.gdxOpenRead(path,error),"read open");open_time=seconds(start);
        start=Clock::now();
        int symbols{},uels{},version{},compression{};
        check(g,g.gdxSystemInfo(symbols,uels),"metadata");check(g,g.gdxFileInfo(version,compression),"file info");
        if (symbols!=w.symbols || uels!=w.labels || version!=7 || compression!=w.compression)
            throw std::runtime_error("workload file metadata differs");
        for (int i=1;i<=uels;++i) {
            char text[GMS_SSSIZE]{};int mapped{};
            check(g,g.gdxUMUelGet(i,text,mapped),"label lookup");
            if (label(w,i)!=text) throw std::runtime_error("label bytes differ");
        }
        for (int i=1;i<=symbols;++i) {
            char name[GMS_SSSIZE]{},description[GMS_SSSIZE]{};int dim{},type{},rows{},user{};
            check(g,g.gdxSymbolInfo(i,name,dim,type),"symbol metadata");
            check(g,g.gdxSymbolInfoX(i,rows,user,description),"symbol extended metadata");
            if (symbol_name(w,i)!=name || std::string(description)!="benchmark" || dim!=w.dimension || type!=w.type || user!=w.user || rows!=(w.symbols==1?w.rows:1))
                throw std::runtime_error("symbol metadata differs");
        }
        metadata_time=seconds(start);start=Clock::now();auto scan_start_cpu=std::clock();
        for (int symbol=1;symbol<=symbols;++symbol) {
            int expected{},keys[20]{},first{};double values[5]{};
            check(g,g.gdxDataReadRawStart(symbol,expected),"scan start");
            std::uint64_t actual=0;
            while (g.gdxDataReadRaw(keys,values,first)) { digest=hash_row(digest,keys,values,w);++count;++actual; }
            check(g,g.gdxDataReadDone(),"scan done");
            if (g.gdxGetLastError() || actual!=static_cast<std::uint64_t>(expected)) throw std::runtime_error("scan incomplete");
        }
        scan_time=seconds(start);scan_cpu=double(std::clock()-scan_start_cpu)/CLOCKS_PER_SEC;
        if (g.gdxClose()) throw std::runtime_error("read close");
    } else throw std::runtime_error("mode must be write or scan");
    if (count!=static_cast<std::uint64_t>(w.rows)) throw std::runtime_error("total row count differs");
    std::ostringstream hex;hex<<std::hex<<std::setw(16)<<std::setfill('0')<<digest;
    std::cout<<std::setprecision(10)<<"{\"schema\":1,\"rows\":"<<count<<",\"digest\":"<<(mode=="scan" ? "\""+hex.str()+"\"" : "null")<<",\"elapsed_seconds\":"<<seconds(wall)
             <<",\"cpu_seconds\":"<<double(std::clock()-cpu)/CLOCKS_PER_SEC<<",\"peak_rss_bytes\":"<<rss()
             <<",\"open_seconds\":"<<open_time<<",\"metadata_seconds\":"<<metadata_time<<",\"scan_seconds\":"<<scan_time
             <<",\"scan_cpu_seconds\":"<<scan_cpu<<",\"registration_seconds\":"<<registration_time
             <<",\"encode_seconds\":"<<encode_time<<",\"finish_seconds\":"<<finish_time<<"}\n";
}
int main(int argc,char **argv) {
    try {
        if (argc!=4) throw std::runtime_error("usage: gdxrs-bench {write|scan} FILE WORKLOAD");
        execute(argv[1],argv[2],workload(argv[3]));return 0;
    } catch(const std::exception &error) { std::cerr<<"benchmark: "<<error.what()<<'\n';return 1; }
}
