// Separate qualification driver; preserve the frozen fixture generator verbatim.
#define main frozen_oracle_main
#include "../oracle/driver.cpp"
#undef main
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
int main(int argc,char **argv) {
    try {
        if(argc!=5 || std::string(argv[1])!="mapped-dump")throw std::runtime_error("usage: mapped-dump FILE MAP SYMBOL");
        mapped_dump(argv[2],argv[3],std::stoi(argv[4]));return 0;
    }catch(const std::exception &e){std::cerr<<"mapped oracle: "<<e.what()<<'\n';return 1;}
}
