// Development-only streaming v7 generator and pinned native validating sink.
#include "gdx.hpp"
#include <array>
#include <algorithm>
#include <cstdint>
#include <cstring>
#include <fstream>
#include <iomanip>
#include <iostream>
#include <stdexcept>
#include <string>
#include <vector>
#include <chrono>
using U = std::uint64_t;
constexpr U SEED=0x1640;
constexpr U INITIAL=14695981039346656037ULL;
U update(U h,U word){return ((h<<7)|(h>>57))^word;}
U step(U h,U word){return update(h,word)*0x9e3779b185ebca87ULL;}
U bits(U n,int f){return 0x3ff0000000000000ULL | ((n*17+U(f)*12345+SEED)&0xfffffffffffffULL);}
std::string label(U n){std::string s="u"+std::to_string(1000000000ULL+n);s.resize(63,'x');return s;}
struct Summary{U count=0,digest=INITIAL;void row(U n){for(int d=0;d<20;++d)digest=step(digest,d==0?n/1024+1:(d==19?n%1024+1:1));for(int f=0;f<5;++f)digest=step(digest,bits(n,f));++count;}};
class Output {
    std::fstream file;
    std::array<char,1024*1024> buffer{};
public:
    explicit Output(const std::string &path){file.rdbuf()->pubsetbuf(buffer.data(),buffer.size());file.open(path,std::ios::binary|std::ios::out|std::ios::in|std::ios::trunc);if(!file)throw std::runtime_error("output open");}
    void bytes(const void *p,std::size_t n){file.write(static_cast<const char*>(p),n);if(!file)throw std::runtime_error("output write");}
    void integer(U n,int width){char b[8];for(int i=0;i<width;++i){b[i]=char(n&255);n>>=8;}bytes(b,width);}
    void str(const std::string &s){if(s.size()>255)throw std::runtime_error("string too long");integer(s.size(),1);bytes(s.data(),s.size());}
    U position(){auto p=file.tellp();if(p<0)throw std::runtime_error("tell");return U(p);}
    void seek(U p){file.seekp(p);if(!file)throw std::runtime_error("seek");}
    void close(){file.flush();if(!file)throw std::runtime_error("flush");file.close();if(file.fail())throw std::runtime_error("close");}
};
void summary(const char *name,const Summary &s,bool comma=true){std::cout<<'"'<<name<<"\":{\"count\":"<<s.count<<",\"digest\":\""<<std::hex<<std::setw(16)<<std::setfill('0')<<s.digest<<std::dec<<"\"}"<<(comma?",":"");}
void generate(const std::string &path,const std::string &kind,U count){
    if(!count || count>1000000000ULL)throw std::runtime_error("invalid development count");
    bool records=kind=="records";if(!records && kind!="metadata")throw std::runtime_error("kind");
    U labels=records?std::max<U>(1024,(count+1023)/1024):count;
    Output out(path);out.integer(2,1);out.integer(0x1234,2);out.integer(4,1);out.integer(0x12345678,4);out.integer(8,1);out.integer(0x400921fb54442d18ULL,8);
    out.integer(123,1);out.str("GAMSGDX");out.integer(7,4);out.integer(0,4);out.str("gdxrs-scale-016");out.str("deterministic-streaming-dev-generator");
    U index=out.position();for(int i=0;i<10;++i)out.integer(0,8);U data=out.position();
    Summary all,leading,nonleading,head;
    if(records){out.str("_DATA_");out.integer(20,1);out.integer(0xffffffffULL,4);for(int d=0;d<20;++d){out.integer(0,4);out.integer(0x7fffffff,4);}
        std::array<char,126> frame{};frame[0]=1;
        for(U n=0;n<count;++n){std::size_t at=1;for(int d=0;d<20;++d){U key=d==0?n/1024+1:(d==19?n%1024+1:1);for(int b=0;b<4;++b){frame[at++]=char(key&255);key>>=8;}}
            for(int f=0;f<5;++f){frame[at++]=10;U v=bits(n,f);for(int b=0;b<8;++b){frame[at++]=char(v&255);v>>=8;}}
            out.bytes(frame.data(),frame.size());all.row(n);if(n/1024+1>=2 && n/1024+1<=3)leading.row(n);if(n%1024+1<=3)nonleading.row(n);if(n<13)head.row(n);
        }out.integer(255,1);
    }
    U symbols=out.position();out.str("_SYMB_");out.integer(records?1:0,4);
    if(records){out.str("data");out.integer(data,8);out.integer(20,4);out.integer(2,1);out.integer(5,4);out.integer(count,4);out.integer(0,4);out.integer(0,1);out.str("scale recipe");out.integer(0,1);out.integer(0,1);out.integer(0,4);}
    out.str("_SYMB_");U texts=out.position();out.str("_SETT_");out.integer(1,4);out.str("");out.str("_SETT_");
    U uels=out.position();out.str("_UEL_");out.integer(labels,4);U label_digest=INITIAL;
    for(U n=1;n<=labels;++n){auto s=label(n);out.str(s);label_digest=step(label_digest,n);for(unsigned char c:s)label_digest=step(label_digest,c);}
    out.str("_UEL_");U acronyms=out.position();out.str("_ACRO_");out.integer(0,4);out.str("_ACRO_");U size=out.position();
    out.seek(index);out.integer(19510624,4);for(U p:{symbols,uels,texts,acronyms,symbols,U(0)})out.integer(p,8);out.close();
    U lookup_count=0,lookup_digest=INITIAL,selected=labels/4;
    for(U n=1;n<=selected;++n){U k=selected-n,ordinal=k+k/16;for(int duplicate=0;duplicate<(k%16==15?2:1);++duplicate){lookup_digest=step(step(lookup_digest,ordinal+duplicate),n);for(unsigned char c:label(n))lookup_digest=step(lookup_digest,c);++lookup_count;}}
    lookup_digest=step(step(lookup_digest,selected+selected/16),0);for(unsigned char c:label(labels+1))lookup_digest=step(lookup_digest,c);++lookup_count;
    std::cout<<"{\"lookup_count\":"<<lookup_count<<",\"lookup_digest\":\""<<std::hex<<std::setw(16)<<std::setfill('0')<<lookup_digest<<std::dec<<"\",\"kind\":\""<<kind<<"\",\"count\":"<<count<<",\"labels\":"<<labels<<",\"bytes\":"<<size<<",\"data_start\":"<<data<<",\"data_end\":"<<symbols<<",\"logical_output_bytes\":"<<(records?count*120:labels*64)<<",\"label_digest\":\""<<std::hex<<std::setw(16)<<std::setfill('0')<<label_digest<<std::dec<<"\",\"expected\":{";summary("all",all);summary("leading",leading);summary("nonleading",nonleading);summary("head",head,false);std::cout<<"}}\n";
}
void native_scan(const std::string &path,U count){gdx::TGXFileObj g;int error=0;
    if(!g.gdxOpenRead(path.c_str(),error))throw std::runtime_error("native open "+std::to_string(error));
    U labels=std::max<U>(1024,(count+1023)/1024),label_digest=INITIAL;int symbols=0,uels=0;
    if(!g.gdxSystemInfo(symbols,uels)||symbols!=1||U(uels)!=labels)throw std::runtime_error("native record metadata counts");
    for(U n=1;n<=labels;++n){char text[GMS_SSSIZE]{};int mapped=0;if(!g.gdxUMUelGet(int(n),text,mapped)||std::string(text)!=label(n))throw std::runtime_error("native record UEL IDs");label_digest=step(label_digest,n);for(unsigned char c:label(n))label_digest=step(label_digest,c);}
    int rows=0;if(!g.gdxDataReadRawStart(1,rows)||U(rows)!=count)throw std::runtime_error("native count");
    int keys[20]{},changed=0;double values[5]{};Summary result;
    for(U n=0;n<count;++n){if(!g.gdxDataReadRaw(keys,values,changed))throw std::runtime_error("native premature EOF");
        for(int d=0;d<20;++d){U expected=d==0?n/1024+1:(d==19?n%1024+1:1);if(U(keys[d])!=expected)throw std::runtime_error("native key mismatch");result.digest=step(result.digest,U(keys[d]));}
        for(int f=0;f<5;++f){U actual=0;std::memcpy(&actual,&values[f],8);if(actual!=bits(n,f))throw std::runtime_error("native value mismatch");result.digest=step(result.digest,actual);}++result.count;
    }
    if(g.gdxDataReadRaw(keys,values,changed)||!g.gdxDataReadDone()||g.gdxClose())throw std::runtime_error("native finish");
    std::cout<<"{\"label_digest\":\""<<std::hex<<std::setw(16)<<std::setfill('0')<<label_digest<<std::dec<<"\",";summary("all",result,false);std::cout<<"}\n";
}
void native_labels(const std::string &path,U count){gdx::TGXFileObj g;int error=0,symbols=0,uels=0;
    if(!g.gdxOpenRead(path.c_str(),error)||!g.gdxSystemInfo(symbols,uels)||symbols!=0||U(uels)!=count)throw std::runtime_error("native metadata open/count");
    U digest=INITIAL;for(U n=1;n<=count;++n){char text[GMS_SSSIZE]{};int mapped=0;if(!g.gdxUMUelGet(int(n),text,mapped)||std::string(text)!=label(n))throw std::runtime_error("native UEL recipe");digest=step(digest,n);for(unsigned char c:label(n))digest=step(digest,c);}
    if(g.gdxClose())throw std::runtime_error("native metadata close");std::cout<<"{\"count\":"<<count<<",\"digest\":\""<<std::hex<<std::setw(16)<<std::setfill('0')<<digest<<"\"}\n";
}

void native_universe(const std::string &path,U count){gdx::TGXFileObj g;int error=0,symbols=0,uels=0;
    if(!g.gdxOpenRead(path.c_str(),error)||!g.gdxSystemInfo(symbols,uels)||symbols!=0||U(uels)!=count)throw std::runtime_error("native metadata open/count");
    U labels=INITIAL;for(U n=1;n<=count;++n){char text[GMS_SSSIZE]{};int mapped=0;if(!g.gdxUMUelGet(int(n),text,mapped)||std::string(text)!=label(n))throw std::runtime_error("native UEL recipe");labels=step(labels,n);for(unsigned char c:label(n))labels=step(labels,c);}
    int rows=0;if(!g.gdxDataReadRawStart(0,rows)||U(rows)!=count)throw std::runtime_error("native universe start");
    int keys[20]{},changed=0;double values[5]{};U digest=INITIAL;
    for(U n=1;n<=count;++n){if(!g.gdxDataReadRaw(keys,values,changed)||U(keys[0])!=n)throw std::runtime_error("native universe ID");U value=0;std::memcpy(&value,&values[0],8);if(value)throw std::runtime_error("native universe value");digest=step(step(digest,n),value);}
    if(g.gdxDataReadRaw(keys,values,changed)||!g.gdxDataReadDone()||g.gdxClose())throw std::runtime_error("native universe finish");
    std::cout<<"{\"count\":"<<count<<",\"label_digest\":\""<<std::hex<<std::setw(16)<<std::setfill('0')<<labels<<"\",\"digest\":\""<<std::setw(16)<<digest<<"\"}\n";
}

void native_write(const std::string &path,const std::string &kind,U count,bool compression){
    using Clock=std::chrono::steady_clock;
    auto seconds=[](auto start){return std::chrono::duration<double>(Clock::now()-start).count();};
    gdx::TGXFileObj g;int error=0;
    if(!g.gdxOpenWriteEx(path.c_str(),"gdxrs-native-writer-023",compression?1:0,error))throw std::runtime_error("native writer open");g.gdxAutoConvert(0);
    U labels=kind=="metadata"?count:std::max<U>(1024,(count+1023)/1024);
    auto start=Clock::now();
    if(!g.gdxUELRegisterRawStart())throw std::runtime_error("native registration start");
    U label_digest=INITIAL;
    for(U n=1;n<=labels;++n){auto text=label(n);if(!g.gdxUELRegisterRaw(text.c_str()))throw std::runtime_error("native registration");label_digest=step(label_digest,n);for(unsigned char c:text)label_digest=step(label_digest,c);}
    if(!g.gdxUELRegisterDone())throw std::runtime_error("native registration finish");
    double registration=seconds(start);start=Clock::now();Summary result;
    if(kind!="metadata"){
        if(!g.gdxDataWriteRawStart("data","scale recipe",20,GMS_DT_VAR,5))throw std::runtime_error("native data start");
        int keys[20]{};double values[5]{};
        for(U n=0;n<count;++n){for(int d=0;d<20;++d)keys[d]=int(d==0?n/1024+1:(d==19?n%1024+1:1));for(int f=0;f<5;++f){U b=bits(n,f);std::memcpy(&values[f],&b,8);}if(!g.gdxDataWriteRaw(keys,values))throw std::runtime_error("native data row");result.row(n);}
        if(!g.gdxDataWriteDone()||g.gdxDataErrorCount())throw std::runtime_error("native data finish");
    }
    double encoding=seconds(start);start=Clock::now();if(g.gdxClose())throw std::runtime_error("native writer close");double finalize=seconds(start);
    std::cout<<"{\"registration_seconds\":"<<registration<<",\"encoding_seconds\":"<<encoding<<",\"finalization_seconds\":"<<finalize<<",\"labels\":"<<labels<<",\"label_digest\":\""<<std::hex<<std::setw(16)<<std::setfill('0')<<label_digest<<std::dec<<"\",";summary("all",result,false);std::cout<<"}\n";
}
int main(int argc,char **argv){try{if(argc==6 && std::string(argv[1])=="write"){native_write(argv[2],argv[3],std::stoull(argv[4]),std::string(argv[5])=="zlib");return 0;}if(argc!=5)throw std::runtime_error("usage: generate|native PATH records|metadata COUNT");U n=std::stoull(argv[4]);if(std::string(argv[1])=="generate")generate(argv[2],argv[3],n);else if(std::string(argv[1])=="native")native_scan(argv[2],n);else if(std::string(argv[1])=="native-universe")native_universe(argv[2],n);else if(std::string(argv[1])=="native-labels")native_labels(argv[2],n);else throw std::runtime_error("mode");return 0;}catch(const std::exception &e){std::cerr<<e.what()<<'\n';return 1;}}
