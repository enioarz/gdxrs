// Development-only append reference; native append is in-place, unlike Rust.
#define main frozen_oracle_main
#include "../oracle/driver.cpp"
#undef main
static void append(const char *path) {
    TGXFileObj g;int error{};
    require(g,g.gdxOpenAppend(path,"gdxrs-append",error),"append open");g.gdxAutoConvert(0);
    require(g,g.gdxUELRegisterRawStart(),"raw start");require(g,g.gdxUELRegisterRaw("omega"),"omega");require(g,g.gdxUELRegisterDone(),"raw done");
    int text{};require(g,g.gdxAddSetText("new text",text),"new text");if(text!=3)throw std::runtime_error("new text ID");
    require(g,g.gdxAcronymAdd("AddedAcro","appended",9),"new acronym");
    int keys[20]{5};double values[5]{3};
    require(g,g.gdxDataWriteRawStart("NewSet","",1,GMS_DT_SET,0),"set start");require(g,g.gdxDataWriteRaw(keys,values),"set row");require(g,g.gdxDataWriteDone(),"set done");
    require(g,g.gdxDataWriteRawStart("NewParam","",2,GMS_DT_PAR,0),"param start");const char *domains[]={"J","NewSet"};require(g,g.gdxSymbolSetDomain(domains),"new strict domains");keys[0]=1;keys[1]=5;values[0]=9*1e301;require(g,g.gdxDataWriteRaw(keys,values),"param row");require(g,g.gdxDataWriteDone(),"param done");require(g,g.gdxSymbolAddComment(7,"appended comment"),"new comment");
    require(g,g.gdxAddAlias("NewSet","NewAlias"),"new alias");
    if(g.gdxClose())throw std::runtime_error("append close");
}
int main(int argc,char **argv) {
    try {
        if(argc==3 && std::string(argv[1])=="append")append(argv[2]);
        else throw std::runtime_error("usage: append FILE");
        return 0;
    }catch(const std::exception&e){std::cerr<<"append oracle: "<<e.what()<<'\n';return 1;}
}
