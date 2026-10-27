// Task-028 sidecar. Do not modify the frozen fixture generator.
#define main frozen_oracle_main
#include "../oracle/driver.cpp"
#undef main
#include <cmath>
#include <limits>
static double from_bits(std::uint64_t bits) {double v{};std::memcpy(&v,&bits,8);return v;}
static void bit_text(double v) {std::uint64_t b{};std::memcpy(&b,&v,8);std::cout<<std::hex<<std::setw(16)<<std::setfill('0')<<b<<std::dec;}
static void author(const char *path,int compression,bool bit_map) {
    TGXFileObj g;int error{};require(g,g.gdxOpenWriteEx(path,"gdxrs-values",compression,error),"open");g.gdxAutoConvert(0);
    if(g.gdxAcronymAdd("Old","before",7)!=1 || g.gdxAcronymAdd("Beta","second",9)!=2)throw std::runtime_error("acronym add");require(g,g.gdxAcronymSetInfo(1,"Alpha","renamed",7),"rename");
    double specials[7]{};g.gdxGetSpecialValues(specials);
    const double custom[]={-101,-102,-103,-104,-105};const double sentinels[]={from_bits(0x7ff8000000001234ULL),-0.0,0.0,std::numeric_limits<double>::infinity(),-std::numeric_limits<double>::infinity()};
    for(int f=0;f<5;++f)specials[f]=bit_map?sentinels[f]:custom[f];require(g,g.gdxSetSpecialValues(specials),"custom write map");
    require(g,g.gdxUELRegisterRawStart(),"UEL start");for(auto label:{"a","b","c"})require(g,g.gdxUELRegisterRaw(label),"UEL");require(g,g.gdxUELRegisterDone(),"UEL done");
    require(g,g.gdxDataWriteRawStart("V","",1,GMS_DT_VAR,GMS_VARTYPE_FREE),"V start");int keys[20]{1};double values[5]{};
    for(int f=0;f<5;++f)values[f]=specials[f];require(g,g.gdxDataWriteRaw(keys,values),"special row");keys[0]=2;
    const double ieee[]={1.25,-0.0,from_bits(0x7ff8000000001234ULL),std::numeric_limits<double>::infinity(),-std::numeric_limits<double>::infinity()};
    for(int f=0;f<5;++f)values[f]=bit_map?8.0+f:ieee[f];require(g,g.gdxDataWriteRaw(keys,values),"IEEE row");keys[0]=3;
    values[0]=g.gdxAcronymValue(7);values[1]=g.gdxAcronymValue(9);values[2]=42;values[3]=specials[1];values[4]=specials[4];require(g,g.gdxDataWriteRaw(keys,values),"acronym row");require(g,g.gdxDataWriteDone(),"V done");if(g.gdxClose())throw std::runtime_error("close");
}
static void project(const char *path,const std::string &mode) {
    TGXFileObj g;int error{};require(g,g.gdxOpenRead(path,error),"open read");double specials[7]{};g.gdxGetSpecialValues(specials);
    if(mode!="identity") {
        for(int f=0;f<5;++f)specials[f]=-101-f;require(g,g.gdxSetSpecialValues(specials),"shared special map");
        for(int f=0;f<5;++f)specials[f]=mode=="loss"?from_bits(0x7ff8000000001234ULL):-201-f;require(g,g.gdxSetReadSpecialValues(specials),"read map");
        require(g,g.gdxAcronymSetInfo(1,"Alpha","ignored",70),"read remap");
        if(g.gdxAcronymSetInfo(2,"Beta","ignored",70))throw std::runtime_error("explicit acronym collision accepted");
        g.gdxAcronymNextNr(mode=="collision"?70:100);
    }
    if(mode=="reset")require(g,g.gdxResetSpecialValues(),"reset");if(mode=="loss")g.gdxMapAcronymsToNaNSet(1);
    int count{},keys[20]{},first{};double values[5]{};require(g,g.gdxDataReadRawStart(1,count),"read start");std::cout<<"GDXRS-VALUE-PROJECTION\t1\n";
    while(g.gdxDataReadRaw(keys,values,first)) {std::cout<<"R\t"<<keys[0]<<'\t';for(int f=0;f<5;++f){if(f)std::cout<<',';bit_text(values[f]);}std::cout<<'\n';}
    require(g,g.gdxDataReadDone(),"read done");std::cout<<"N\t"<<g.gdxAcronymNextNr(-1)<<'\n';
    if(g.gdxClose())throw std::runtime_error("read close");
}
static void probes() {
    TGXFileObj g;double before[7]{},duplicate[7]{},after[7]{};g.gdxGetSpecialValues(before);std::copy(before,before+7,duplicate);duplicate[1]=duplicate[0];
    if(g.gdxSetSpecialValues(duplicate))throw std::runtime_error("duplicate write sentinels accepted");g.gdxGetSpecialValues(after);for(int f=0;f<5;++f)if(before[f]!=after[f])throw std::runtime_error("non-atomic write policy update");
    require(g,g.gdxSetReadSpecialValues(duplicate),"duplicate read allowed");require(g,g.gdxResetSpecialValues(),"reset");
    std::cout<<"write_duplicate_rejected\t1\nread_duplicate_allowed\t1\nreset\t1\nacronym_overflow_is_infinity\t"<<std::isinf(g.gdxAcronymValue(std::numeric_limits<int>::max()))<<'\n';
}
int main(int argc,char **argv) {try {if(argc==2 && std::string(argv[1])=="probes")probes();else if(argc==5 && std::string(argv[1])=="author")author(argv[2],std::stoi(argv[3]),std::string(argv[4])=="bits");else if(argc==4 && std::string(argv[1])=="project")project(argv[2],argv[3]);else throw std::runtime_error("invalid value-helper arguments");return 0;}catch(const std::exception &e){std::cerr<<"value oracle: "<<e.what()<<'\n';return 1;}}
