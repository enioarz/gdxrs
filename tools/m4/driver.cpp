// Development-only task-031 independent generator, native sink and eager reference.
#define main prior_scale_main
#include "../scale/driver.cpp"
#undef main
#include <unordered_map>
namespace m4 {
constexpr U LABELS=65536;
using Clock=std::chrono::steady_clock;
double seconds(Clock::time_point t){return std::chrono::duration<double>(Clock::now()-t).count();}
bool dictionary(const std::string &k){return k=="dictionary";}
int dim(const std::string &k){return k=="strings"?2:dictionary(k)?1:20;}
U labels(const std::string &k,U n){return dictionary(k)?n:LABELS+(k=="errors");}
std::string text(const std::string &k,U n){auto s=::label(n);if(k=="strings")s.resize(11);return s;}
int caller(U n){return n==1?0:std::numeric_limits<int>::max()-int(31*(n-2));}
std::array<int,20> keys(U n,int dimension){std::array<int,20> k{};k.fill(1);k[0]=int(n/65536+1);k[1]=int(n%65536+1);for(int d=2;d<dimension;++d)k[d]=int((n*U(2*d+1)+SEED)&65535)+1;return k;}
std::array<U,5> selected(U n){return {1,2,n/2,n-1,n};}
struct Digest {U count=0,hash=INITIAL;void row(const std::array<int,20>&k,U n,int dimension,int fields){for(int d=0;d<dimension;++d)hash=step(hash,U(k[d]));for(int f=0;f<fields;++f)hash=step(hash,::bits(n,f));++count;}};
U label_hash(const std::string &kind,U n){U h=INITIAL;for(U i=1;i<=labels(kind,n);++i){h=step(h,i);for(unsigned char c:text(kind,i))h=step(h,c);}return h;}
U ordinal(U n,U total,U tag){U base=total-1-n+((total+1)/2-(n+2)/2)+((total+2)/3-(n+3)/3)+1;return base+(tag==1?1:1+(n%2==0));}
U diagnostic(U h,U n,U total,U tag){h=step(step(step(h,tag),ordinal(n,total,tag)),tag==1?1:0);auto k=keys(n,20);if(tag==1)k[0]=int(LABELS+1);for(int v:k)h=step(h,U(v));for(int f=0;f<5;++f)h=step(h,::bits(n,f));return h;}
void expected(const std::string &kind,U n){Digest d;if(dictionary(kind)){for(U raw:selected(n)){auto k=keys(0,1);k[0]=int(raw);d.row(k,raw,1,1);}}else{for(U i=0;i<n;++i)d.row(keys(i,dim(kind)),i,dim(kind),5);}U errors=0,h=INITIAL;if(kind=="errors"){
 for(U i=0;i<n;i+=3){h=diagnostic(h,i,n,2);++errors;}
 for(U low=0;low<LABELS;low+=2){if(low>=n)break;U i=(n-1-low)/LABELS*LABELS+low;for(;;){h=diagnostic(h,i,n,1);++errors;if(i<LABELS)break;i-=LABELS;}}
 }std::cout<<"{\"count\":"<<d.count<<",\"digest\":\""<<std::hex<<std::setw(16)<<std::setfill('0')<<d.hash<<"\",\"label_digest\":\""<<std::setw(16)<<label_hash(kind,n)<<"\",\"error_digest\":\""<<std::setw(16)<<h<<std::dec<<"\",\"labels\":"<<labels(kind,n)<<",\"errors\":"<<errors<<",\"error_bytes\":"<<errors*159<<"}\n";}
void check(gdx::TGXFileObj &g,int ok,const char *what){if(!ok)throw std::runtime_error(std::string(what)+" (native "+std::to_string(g.gdxGetLastError())+")");}
void verify(const std::string &path,const std::string &kind,U n,bool emit=true){gdx::TGXFileObj g;int error{},symbols{},uels{};check(g,g.gdxOpenRead(path.c_str(),error),"open");check(g,g.gdxSystemInfo(symbols,uels),"system info");if(symbols!=(kind=="errors"||kind=="append"?2:1)||U(uels)!=labels(kind,n))throw std::runtime_error("native catalog counts");U lh=INITIAL;
 for(U i=1;i<=U(uels);++i){char b[GMS_SSSIZE]{};int mapped{};check(g,g.gdxUMUelGet(int(i),b,mapped),"UEL");if(std::string(b)!=text(kind,i))throw std::runtime_error("native UEL identity");lh=step(lh,i);for(unsigned char c:std::string(b))lh=step(lh,c);}
 if(kind=="errors"){int count{};check(g,g.gdxDataReadRawStart(1,count),"set start");if(count!=int(LABELS))throw std::runtime_error("set count");int k[20]{},changed{};double v[5]{};for(U i=1;i<=LABELS;++i){check(g,g.gdxDataReadRaw(k,v,changed),"set row");if(k[0]!=int(i)||v[0]!=0)throw std::runtime_error("set identity");}if(g.gdxDataReadRaw(k,v,changed))throw std::runtime_error("extra set row");check(g,g.gdxDataReadDone(),"set done");}
 int id=kind=="errors"?2:1,dimension{},type{},count{},user{};char name[GMS_SSSIZE]{},desc[GMS_SSSIZE]{};check(g,g.gdxSymbolInfo(id,name,dimension,type),"symbol");check(g,g.gdxSymbolInfoX(id,count,user,desc),"symbol X");U wanted=dictionary(kind)?5:n;
 if(std::string(name)!="data"||dimension!=dim(kind)||type!=(dictionary(kind)?GMS_DT_PAR:GMS_DT_VAR)||user!=(dictionary(kind)?0:5)||U(count)!=wanted)throw std::runtime_error("native symbol metadata");int domains[20]{};check(g,g.gdxSymbolGetDomain(id,domains),"domains");for(int d=0;d<dimension;++d)if(domains[d]!=(kind=="errors"&&d==0?1:0))throw std::runtime_error("native domain ID");
 int expected_count{};check(g,g.gdxDataReadRawStart(id,expected_count),"data start");if(U(expected_count)!=wanted)throw std::runtime_error("native raw count");Digest digest;auto sparse=selected(n);
 for(U i=0;i<wanted;++i){int k[20]{},changed{};double values[5]{};check(g,g.gdxDataReadRaw(k,values,changed),"row");U index=dictionary(kind)?sparse[std::size_t(i)]:i;auto want=keys(index,dimension);if(dictionary(kind))want[0]=int(index);for(int d=0;d<dimension;++d){if(k[d]!=want[d])throw std::runtime_error("native ordered keys");digest.hash=step(digest.hash,U(k[d]));}for(int f=0;f<(dictionary(kind)?1:5);++f){U b{};std::memcpy(&b,&values[f],8);if(b!=::bits(index,f))throw std::runtime_error("native ordinary bits");digest.hash=step(digest.hash,b);}++digest.count;
 }int k[20]{},changed{};double v[5]{};if(g.gdxDataReadRaw(k,v,changed))throw std::runtime_error("extra record");check(g,g.gdxDataReadDone(),"data done");
 if(kind=="append"){check(g,g.gdxSymbolInfo(2,name,dimension,type),"append symbol");if(std::string(name)!="added"||dimension!=0||type!=GMS_DT_PAR)throw std::runtime_error("append metadata ID");check(g,g.gdxDataReadRawStart(2,count),"append read");check(g,g.gdxDataReadRaw(k,v,changed),"append row");if(count!=1||v[0]!=42||g.gdxDataReadRaw(k,v,changed))throw std::runtime_error("append value/count");check(g,g.gdxDataReadDone(),"append done");}
 if(g.gdxGetLastError()||g.gdxClose())throw std::runtime_error("native finish");if(emit)std::cout<<"{\"count\":"<<digest.count<<",\"digest\":\""<<std::hex<<std::setw(16)<<std::setfill('0')<<digest.hash<<"\",\"label_digest\":\""<<std::setw(16)<<lh<<std::dec<<"\"}\n";
}
void generate(const std::string &path,U n){Output out(path);out.integer(2,1);out.integer(0x1234,2);out.integer(4,1);out.integer(0x12345678,4);out.integer(8,1);out.integer(0x400921fb54442d18ULL,8);out.integer(123,1);out.str("GAMSGDX");out.integer(7,4);out.integer(0,4);out.str("gdxrs-m4-generator");out.str("independent-streaming-generator");U index=out.position();for(int i=0;i<10;++i)out.integer(0,8);U data=out.position();out.str("_DATA_");out.integer(20,1);out.integer(0xffffffff,4);for(int d=0;d<20;++d){out.integer(0,4);out.integer(0x7fffffff,4);}std::array<char,126> frame{};frame[0]=1;
 for(U i=0;i<n;++i){auto k=keys(i,20);std::size_t at=1;for(int value:k){U v=U(value);for(int b=0;b<4;++b){frame[at++]=char(v&255);v>>=8;}}for(int f=0;f<5;++f){frame[at++]=10;U v=::bits(i,f);for(int b=0;b<8;++b){frame[at++]=char(v&255);v>>=8;}}out.bytes(frame.data(),frame.size());}out.integer(255,1);U syms=out.position();out.str("_SYMB_");out.integer(1,4);out.str("data");out.integer(data,8);out.integer(20,4);out.integer(GMS_DT_VAR,1);out.integer(5,4);out.integer(n,4);out.integer(0,4);out.integer(0,1);out.str("");out.integer(0,1);out.integer(0,1);out.integer(0,4);out.str("_SYMB_");U texts=out.position();out.str("_SETT_");out.integer(1,4);out.str("");out.str("_SETT_");U ul=out.position();out.str("_UEL_");out.integer(LABELS,4);for(U i=1;i<=LABELS;++i)out.str(text("unsorted",i));out.str("_UEL_");U acro=out.position();out.str("_ACRO_");out.integer(0,4);out.str("_ACRO_");U bytes=out.position();out.seek(index);out.integer(19510624,4);for(U p:{syms,ul,texts,acro,syms,U(0)})out.integer(p,8);out.close();std::cout<<"{\"bytes\":"<<bytes<<",\"count\":"<<n<<"}\n";}
struct Row {std::array<int,20> keys;std::array<U,5> values;U ordinal;};
void write(const std::string &path,const std::string &kind,U n,bool compression,const std::string &input){
 U reported_errors=0,reported_digest=INITIAL;double registration=0,ingestion=0,merge=0,encoding=0,copy=0,finalization=0;auto start=Clock::now();
 if(kind=="append"){
  verify(input,"unsorted",n,false);std::ifstream from(input,std::ios::binary);std::ofstream to(path,std::ios::binary|std::ios::out);std::array<char,65536> buffer{};while(from){from.read(buffer.data(),buffer.size());auto got=from.gcount();if(got)to.write(buffer.data(),got);}to.flush();to.close();if(!from.eof()||!to)throw std::runtime_error("bounded native copy");copy=seconds(start);start=Clock::now();gdx::TGXFileObj g;int e{};check(g,g.gdxOpenAppend(path.c_str(),"native-m4-copy-reference",e),"append open");check(g,g.gdxDataWriteRawStart("added","",0,GMS_DT_PAR,0),"append start");int k[20]{};double v[5]{42};check(g,g.gdxDataWriteRaw(k,v),"append write");check(g,g.gdxDataWriteDone(),"append done");if(g.gdxClose())throw std::runtime_error("append close");finalization=seconds(start);
 }else{
  gdx::TGXFileObj g;int e{};check(g,g.gdxOpenWriteEx(path.c_str(),"native-m4-composite",compression?1:0,e),"open write");g.gdxAutoConvert(0);std::unordered_map<int,int> map;check(g,g.gdxUELRegisterRawStart(),"register start");U total=labels(kind,n);for(U i=1;i<=total;++i){auto s=text(kind,i);check(g,g.gdxUELRegisterRaw(s.c_str()),"UEL write");if(kind=="mapped"||dictionary(kind))map.emplace(caller(i),int(i));}check(g,g.gdxUELRegisterDone(),"register done");
  if(kind=="errors"){check(g,g.gdxDataWriteRawStart("I","",1,GMS_DT_SET,0),"set start");int k[20]{};double v[5]{};for(U i=1;i<=LABELS;++i){k[0]=int(i);check(g,g.gdxDataWriteRaw(k,v),"set write");}check(g,g.gdxDataWriteDone(),"set finish");}
  registration=seconds(start);start=Clock::now();std::vector<Row> rows;U count=dictionary(kind)?5:n;U input_rows=0;rows.reserve(std::size_t(count+(kind=="errors"?(n+1)/2+(n+2)/3:0)));auto sparse=selected(n);for(U at=count;at>0;--at){U i=dictionary(kind)?sparse[std::size_t(at-1)]:at-1;auto k=keys(i,dim(kind));if(dictionary(kind))k[0]=int(i);
   if(kind=="mapped"||dictionary(kind))for(int d=0;d<dim(kind);++d)k[d]=map.at(caller(U(k[d])));
   if(kind=="strings")for(int d=0;d<2;++d){int raw{},mapped{};auto s=text(kind,U(k[d]));check(g,g.gdxUMFindUEL(s.c_str(),raw,mapped),"string remap");k[d]=raw;}
   std::array<U,5> values{};for(int f=0;f<5;++f)values[f]=::bits(i,f);
   rows.push_back({k,values,++input_rows});
   if(kind=="errors"){if(i%2==0){auto bad=k;bad[0]=int(LABELS+1);rows.push_back({bad,values,++input_rows});}if(i%3==0)rows.push_back({k,values,++input_rows});}
  }ingestion=seconds(start);start=Clock::now();std::sort(rows.begin(),rows.end(),[](const Row&a,const Row&b){return a.keys<b.keys||(a.keys==b.keys&&a.ordinal<b.ordinal);});merge=seconds(start);start=Clock::now();check(g,g.gdxDataWriteRawStart("data","",dim(kind),dictionary(kind)?GMS_DT_PAR:GMS_DT_VAR,dictionary(kind)?0:5),"data start");if(kind=="errors"){const char *domains[20];for(auto &d:domains)d="*";domains[0]="I";check(g,g.gdxSymbolSetDomain(domains),"strict domain");}
  U rejected=0,diagnostic_hash=INITIAL;std::array<int,20> previous{};bool have_previous=false;
  for(const auto &row:rows){
   U tag=kind=="errors"&&row.keys[0]>int(LABELS)?1:kind=="errors"&&have_previous&&row.keys==previous?2:0;
   if(tag){diagnostic_hash=step(step(step(diagnostic_hash,tag),row.ordinal),tag==1?1:0);for(int key:row.keys)diagnostic_hash=step(diagnostic_hash,U(key));for(U value:row.values)diagnostic_hash=step(diagnostic_hash,value);++rejected;continue;}
   double v[5]{};for(int f=0;f<(dictionary(kind)?1:5);++f)std::memcpy(&v[f],&row.values[f],8);
   check(g,g.gdxDataWriteRaw(row.keys.data(),v),"raw encode");previous=row.keys;have_previous=true;
  }
  reported_errors=rejected;reported_digest=diagnostic_hash;
  if(kind=="errors"){U expected_count=(n+1)/2+(n+2)/3;if(rejected!=expected_count)throw std::runtime_error("composite rejected count");}
  check(g,g.gdxDataWriteDone(),"data finish");encoding=seconds(start);start=Clock::now();if(g.gdxClose())throw std::runtime_error("native close");finalization=seconds(start);
 }
 std::cout<<"{\"errors\":"<<reported_errors<<",\"error_digest\":\""<<std::hex<<std::setw(16)<<std::setfill('0')<<reported_digest<<std::dec<<"\",\"registration_seconds\":"<<registration<<",\"remapping_seconds\":0,\"ingestion_seconds\":"<<ingestion<<",\"merge_seconds\":"<<merge<<",\"replay_encoding_seconds\":"<<encoding<<",\"copy_seconds\":"<<copy<<",\"finalization_seconds\":"<<finalization<<"}\n";
}
}
namespace {
U checked_count(const char *s) {
 std::string text(s); if(text.empty()||text.find_first_not_of("0123456789")!=std::string::npos)throw std::runtime_error("decimal count required");
 U n=std::stoull(text);if(n<8||n>60000000)throw std::runtime_error("count outside bounded recipe");return n;
}
std::string checked_kind(const char *s) {
 std::string kind(s);if(kind!="unsorted"&&kind!="strings"&&kind!="mapped"&&kind!="dictionary"&&kind!="errors"&&kind!="append")throw std::runtime_error("unknown M4 family");return kind;
}
}
int main(int argc,char **argv){try{
 if(argc==4&&std::string(argv[1])=="generate"){m4::generate(argv[2],checked_count(argv[3]));return 0;}
 if(argc==4&&std::string(argv[1])=="expected"){m4::expected(checked_kind(argv[2]),checked_count(argv[3]));return 0;}
 if(argc==5&&std::string(argv[1])=="verify"){m4::verify(argv[2],checked_kind(argv[3]),checked_count(argv[4]));return 0;}
 if((argc==6||argc==7)&&std::string(argv[1])=="write"){
  auto kind=checked_kind(argv[3]);std::string codec(argv[5]);if(codec!="raw"&&codec!="zlib")throw std::runtime_error("unknown codec");
  if((kind=="append")!=(argc==7))throw std::runtime_error("append input required exclusively");
  m4::write(argv[2],kind,checked_count(argv[4]),codec=="zlib",argc==7?argv[6]:"");return 0;
 }
 throw std::runtime_error("m4 reference arguments");
}catch(const std::exception&e){std::cerr<<"m4 oracle: "<<e.what()<<'\n';return 1;}}
