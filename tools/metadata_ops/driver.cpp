// Development-only task-029 helper. Frozen fixture generator stays unchanged.
#define main frozen_oracle_main
#include "../oracle/driver.cpp"
#undef main
#include <limits>
static void author(const char *path,int compression) {
    TGXFileObj g;int error{};require(g,g.gdxOpenWriteEx(path,"gdxrs-metadata-ops",compression,error),"open");g.gdxAutoConvert(0);
    require(g,g.gdxUELRegisterMapStart(),"map start");for(auto [id,label]:{std::pair{0,"a"},std::pair{9,"b"},std::pair{3,"c"}})require(g,g.gdxUELRegisterMap(id,label),"map UEL");require(g,g.gdxUELRegisterDone(),"map done");require(g,g.gdxUELRegisterRawStart(),"raw start");const auto longest=std::string(63,'Z');require(g,g.gdxUELRegisterRaw(longest.c_str()),"long UEL");require(g,g.gdxUELRegisterDone(),"raw done");
    int text{};require(g,g.gdxAddSetText("element text",text),"text");if(text!=1)throw std::runtime_error("text ID");require(g,g.gdxAddSetText("spare",text),"spare");
    require(g,g.gdxSetTextNodeNr(0,std::numeric_limits<int>::max()),"node zero");require(g,g.gdxSetTextNodeNr(1,std::numeric_limits<int>::min()),"negative node");if(g.gdxSetTextNodeNr(1,7))throw std::runtime_error("node overwrite accepted");if(g.gdxSetTextNodeNr(999,1))throw std::runtime_error("invalid node accepted");
    int keys[20]{1};double values[5]{1};require(g,g.gdxDataWriteRawStart("I","",1,GMS_DT_SET,0),"I start");require(g,g.gdxDataWriteRaw(keys,values),"I row 1");keys[0]=2;values[0]=0;require(g,g.gdxDataWriteRaw(keys,values),"I row 2");require(g,g.gdxDataWriteDone(),"I done");require(g,g.gdxSymbolAddComment(1,"after I"),"late comment");require(g,g.gdxAddAlias("I","J"),"alias");
    if(g.gdxRenameUEL("a","alpha-long-label"))throw std::runtime_error("rename after I");
    require(g,g.gdxDataWriteRawStart("K","",1,GMS_DT_SET,0),"K start");keys[0]=3;require(g,g.gdxDataWriteRaw(keys,values),"K row");require(g,g.gdxDataWriteDone(),"K done");if(g.gdxRenameUEL("c","gamma"))throw std::runtime_error("rename after K");
    require(g,g.gdxDataWriteRawStart("P","",2,GMS_DT_PAR,0),"P start");const char *strict[]={"J","I"};require(g,g.gdxSymbolSetDomain(strict),"strict");keys[0]=1;keys[1]=2;values[0]=GMS_SV_EPS;require(g,g.gdxDataWriteRaw(keys,values),"P row 1");keys[0]=2;keys[1]=1;values[0]=1.25;require(g,g.gdxDataWriteRaw(keys,values),"P row 2");require(g,g.gdxDataWriteDone(),"P done");const char *shown[]={"Shown","*"};require(g,g.gdxSymbolSetDomainX(4,shown),"display");const char *again[]={"ShownAgain","*"};require(g,g.gdxSymbolSetDomainX(4,again),"display replacement");require(g,g.gdxSymbolAddComment(4,"late P"),"P comment");
    require(g,g.gdxDataWriteRawStart("R","",0,GMS_DT_PAR,0),"R start");values[0]=42;require(g,g.gdxDataWriteRaw(keys,values),"R row");require(g,g.gdxDataWriteDone(),"R done");if(g.gdxRenameUEL("b","beta-final"))throw std::runtime_error("late rename");
    char label[GMS_SSSIZE]{};int node{};require(g,g.gdxGetElemText(1,label,node),"writer node lookup");std::cout<<"writer_node\t"<<node<<'\n';if(g.gdxClose())throw std::runtime_error("close");
}
static void domain_callback(int raw,int mapped,void*) {std::cout<<"D\t"<<raw<<'\t'<<mapped<<'\n';}
static void context(const char *path,bool view) {
    TGXFileObj g;int error{};require(g,g.gdxOpenRead(path,error),"read");require(g,g.gdxUELRegisterMapStart(),"register read map");for(auto [id,label]:{std::pair{9,"alpha-long-label"},std::pair{0,"beta-final"},std::pair{3,"gamma"}})require(g,g.gdxUELRegisterMap(id,label),"map");require(g,g.gdxUELRegisterDone(),"map done");
    if(view) {if(g.gdxRenameUEL("alpha-long-label","session-label") || g.gdxRenameUEL("gamma","session-gamma"))throw std::runtime_error("view rename");require(g,g.gdxSetTextNodeNr(0,std::numeric_limits<int>::max()),"node0");require(g,g.gdxSetTextNodeNr(1,std::numeric_limits<int>::min()),"node1");if(g.gdxSetTextNodeNr(1,7))throw std::runtime_error("node overwritten");}
    std::cout<<"GDXRS-METADATA-CONTEXT\t1\n";int count{},highest{};require(g,g.gdxUMUelInfo(count,highest),"map info");std::cout<<"M\t"<<count<<'\t'<<highest<<'\n';
    for(int n=1;n<=count;++n) {char label[GMS_SSSIZE]{};int mapped{};require(g,g.gdxUMUelGet(n,label,mapped),"UEL info");std::cout<<"U\t"<<n<<'\t'<<mapped<<'\t'<<hex(label)<<'\n';}
    char label[GMS_SSSIZE]{};require(g,g.gdxGetUEL(9,label),"inverse label");std::cout<<"G\t9\t"<<hex(label)<<'\n';int raw{},mapped{};require(g,g.gdxUMFindUEL(view?"session-label":"alpha-long-label",raw,mapped),"find");std::cout<<"F\t"<<raw<<'\t'<<mapped<<'\n';
    for(int n=0;n<3;++n) {int node{};require(g,g.gdxGetElemText(n,label,node),"text info");std::cout<<"T\t"<<n<<'\t'<<node<<'\t'<<hex(label)<<'\n';}
    for(int n=1;n<=5;++n)std::cout<<"H\t"<<n<<'\t'<<g.gdxSetHasText(n)<<'\n';
    int lengths[20]{};int maximum=g.gdxSymbIndxMaxLength(4,lengths);std::cout<<"L\t"<<g.gdxUELMaxLength()<<'\t'<<g.gdxSymbMaxLength()<<'\t'<<maximum<<'\t'<<lengths[0]<<','<<lengths[1]<<'\n';int elements{};require(g,g.gdxGetDomainElements(4,1,gdx::DOMC_EXPAND,domain_callback,elements,nullptr),"domain visit");std::cout<<"E\t"<<elements<<'\n';
    if(g.gdxClose())throw std::runtime_error("close read");
}
static void probes(const char *path) {
    TGXFileObj g;int error{};require(g,g.gdxOpenRead(path,error),"probe open");
    auto emit=[](const char *name,bool ok){if(!ok)throw std::runtime_error(name);std::cout<<name<<"\t1\n";};
    emit("rename_missing_rejected",g.gdxRenameUEL("missing","fresh")!=0);
    emit("rename_duplicate_rejected",g.gdxRenameUEL("alpha-long-label","beta-final")!=0);
    const auto bad=std::string(64,'X');emit("rename_invalid_rejected",g.gdxRenameUEL("alpha-long-label",bad.c_str())!=0);
    int raw{},mapped{};emit("native_uel_case_folded",g.gdxUMFindUEL("ALPHA-LONG-LABEL",raw,mapped) && raw==1);
    int symbol{};emit("symbol_case_folded",g.gdxFindSymbol("p",symbol) && symbol==4);
    char label[GMS_SSSIZE]{};emit("invalid_uel_rejected",!g.gdxUMUelGet(0,label,mapped));emit("invalid_text_node_rejected",!g.gdxSetTextNodeNr(999,1));
    emit("zero_node_slot_supported",g.gdxSetTextNodeNr(0,std::numeric_limits<int>::max()));emit("nonzero_node_immutable",!g.gdxSetTextNodeNr(0,1));int node{};require(g,g.gdxGetElemText(0,label,node),"node query");emit("sparse_node_preserved",node==std::numeric_limits<int>::max());
    if(g.gdxClose())throw std::runtime_error("probe close");
}
int main(int argc,char **argv) {try {if(argc==3 && std::string(argv[1])=="probes")probes(argv[2]);else if(argc==4 && std::string(argv[1])=="author")author(argv[2],std::stoi(argv[3]));else if(argc==4 && std::string(argv[1])=="context")context(argv[2],std::string(argv[3])=="view");else throw std::runtime_error("usage: author FILE COMPRESSION | context FILE base/view");return 0;}catch(const std::exception&e){std::cerr<<"metadata oracle: "<<e.what()<<'\n';return 1;}}
