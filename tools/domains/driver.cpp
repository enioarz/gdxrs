// Separate task-027 oracle. Frozen fixture generator remains unchanged.
#define main frozen_oracle_main
#include "../oracle/driver.cpp"
#undef main
static void author(const char *path,int compression,int cutoff) {
    TGXFileObj g;int error{};
    require(g,g.gdxOpenWriteEx(path,"gdxrs-domains",compression,error),"open");g.gdxAutoConvert(0);
    require(g,g.gdxUELRegisterRawStart(),"UEL start");for(auto label:{"a","b","c"})require(g,g.gdxUELRegisterRaw(label),"UEL");require(g,g.gdxUELRegisterDone(),"UEL done");
    int keys[20]{};double values[5]{};
    require(g,g.gdxDataWriteRawStart("I","",1,GMS_DT_SET,0),"I start");const char *self[]={"I"};require(g,g.gdxSymbolSetDomain(self),"direct self-domain");keys[0]=1;require(g,g.gdxDataWriteRaw(keys,values),"I row");require(g,g.gdxDataWriteDone(),"I done");
    require(g,g.gdxAddAlias("I","J"),"alias");
    require(g,g.gdxDataWriteRawStart("P","",2,GMS_DT_PAR,0),"P start");const char *domains[]={"J","I"};require(g,g.gdxSymbolSetDomain(domains),"P domains");
    g.gdxSetErrorRecordCutoff(cutoff);
    const int tuples[][2]={{1,1},{1,1},{2,1},{2,1},{2,2},{3,1},{3,3}};
    const double data[]={1.25,GMS_SV_EPS,GMS_SV_NA,GMS_SV_UNDEF,GMS_SV_MINF,GMS_SV_PINF,2.5};
    std::cout<<"GDXRS-DOMAIN-ERRORS\t1\n";
    for(int row=0;row<7;++row) {keys[0]=tuples[row][0];keys[1]=tuples[row][1];values[0]=data[row];int ok=g.gdxDataWriteRaw(keys,values);if(ok!=(row==0))throw std::runtime_error("native accepted/rejected expectation changed");std::cout<<"A\t"<<row+1<<'\t'<<ok<<'\n';}
    require(g,g.gdxDataWriteDone(),"P done");
    for(int n=1;n<=g.gdxDataErrorCount();++n) {
        int signed_keys[20]{},absolute_keys[20]{};double signed_values[5]{},absolute_values[5]{};
        require(g,g.gdxDataErrorRecordX(n,signed_keys,signed_values),"signed error");require(g,g.gdxDataErrorRecord(n,absolute_keys,absolute_values),"absolute error");
        unsigned mask{};for(int d=0;d<2;++d) {if(signed_keys[d]<0)mask|=1u<<d;if(absolute_keys[d]!=std::abs(signed_keys[d]))throw std::runtime_error("absolute identity changed");}if(value(g,signed_values[0])!=value(g,absolute_values[0]))throw std::runtime_error("typed error identity changed");
        std::cout<<"X\t"<<n<<'\t'<<absolute_keys[0]<<','<<absolute_keys[1]<<'\t'<<mask<<'\t'<<value(g,signed_values[0])<<'\n';
    }
    std::cout<<"E\t"<<g.gdxDataErrorCount()<<'\n';
    require(g,g.gdxDataWriteRawStart("R","",1,GMS_DT_PAR,0),"R start");if(g.gdxDataErrorCount())throw std::runtime_error("error list failed to reset");keys[0]=3;values[0]=GMS_SV_EPS;require(g,g.gdxDataWriteRaw(keys,values),"relaxed row");require(g,g.gdxDataWriteDone(),"R done");const char *relaxed[]={"Unregistered"};require(g,g.gdxSymbolSetDomainX(4,relaxed),"relaxed metadata");
    require(g,g.gdxDataWriteRawStart("U","",1,GMS_DT_PAR,0),"U start");const char *universe[]={"*"};require(g,g.gdxSymbolSetDomain(universe),"universe domain");keys[0]=3;values[0]=GMS_SV_NA;require(g,g.gdxDataWriteRaw(keys,values),"universe row");require(g,g.gdxDataWriteDone(),"U done");
    if(g.gdxClose())throw std::runtime_error("close failed");
}
int main(int argc,char **argv) {try {if(argc!=4)throw std::runtime_error("usage: FILE COMPRESSION CUTOFF");author(argv[1],std::stoi(argv[2]),std::stoi(argv[3]));return 0;}catch(const std::exception &e){std::cerr<<"domain oracle: "<<e.what()<<'\n';return 1;}}
