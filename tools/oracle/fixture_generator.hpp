// Development-only, bounded recipe executor for upstream-generated test files.
#include <algorithm>
#include <vector>

static std::vector<std::string> split(const std::string &s, char delimiter = '\t') {
    std::vector<std::string> result;
    std::size_t at = 0;
    for (;;) {
        auto end = s.find(delimiter, at);
        result.push_back(s.substr(at, end == std::string::npos ? end : end - at));
        if (end == std::string::npos) return result;
        at = end + 1;
    }
}

static std::string unhex(const std::string &s) {
    if (s.size() % 2 || s.size() > 510) throw std::runtime_error("invalid recipe byte string");
    std::string result;
    for (std::size_t i = 0; i < s.size(); i += 2) {
        auto part = s.substr(i, 2);
        if (part.find_first_not_of("0123456789abcdef") != std::string::npos)
            throw std::runtime_error("invalid recipe hex");
        char c = static_cast<char>(std::stoi(part, nullptr, 16));
        if (!c) throw std::runtime_error("embedded NUL not supported by C API recipe");
        result.push_back(c);
    }
    return result;
}

static double recipe_value(TGXFileObj &g, const std::string &s) {
    if (s.size() == 18 && s.substr(0, 2) == "f:") {
        auto bits = static_cast<std::uint64_t>(std::stoull(s.substr(2), nullptr, 16));
        double v; std::memcpy(&v, &bits, 8); return v;
    }
    if (s.substr(0, 2) == "a:") return g.gdxAcronymValue(std::stoi(s.substr(2)));
    if (s == "s:undef") return GMS_SV_UNDEF;
    if (s == "s:na") return GMS_SV_NA;
    if (s == "s:pinf") return GMS_SV_PINF;
    if (s == "s:minf") return GMS_SV_MINF;
    if (s == "s:eps") return GMS_SV_EPS;
    throw std::runtime_error("invalid recipe value");
}

static void recipe(const char *path, const char *input, int compression) {
    std::ifstream in(input);
    if (!in) throw std::runtime_error("cannot read recipe");
    TGXFileObj g;
    int error{}, dim{}, symbol = 0, line_no = 0;
    std::string mode;
    require(g, g.gdxOpenWriteEx(path, "gdxrs-fixtures", compression, error), "recipe open");
    g.gdxAutoConvert(0);
    std::string line;
    while (std::getline(in, line)) {
        ++line_no;
        if (line.size() > 16384) throw std::runtime_error("recipe line too large");
        if (line.empty() || line[0] == '#') continue;
        auto f = split(line);
        bool negative = f[0][0] == '!';
        if (negative) f[0].erase(0, 1);
        int ok = 1;
        auto field = [&](std::size_t n) -> const std::string & { return f.at(n); };
        if (f[0] == "U") {
            require(g, g.gdxUELRegisterRawStart(), "recipe U start");
            ok = g.gdxUELRegisterRaw(unhex(field(1)).c_str());
            require(g, g.gdxUELRegisterDone(), "recipe U done");
        } else if (f[0] == "URANGE") {
            int n = std::stoi(field(1));
            if (n < 0 || n > 100000) throw std::runtime_error("recipe U range too large");
            require(g, g.gdxUELRegisterRawStart(), "recipe range start");
            for (int i = 1; i <= n; ++i)
                require(g, g.gdxUELRegisterRaw(("u" + std::to_string(i)).c_str()), "range UEL");
            require(g, g.gdxUELRegisterDone(), "recipe range done");
        } else if (f[0] == "M") {
            require(g, g.gdxUELRegisterMapStart(), "recipe map start");
            ok = g.gdxUELRegisterMap(std::stoi(field(1)), unhex(field(2)).c_str());
            require(g, g.gdxUELRegisterDone(), "recipe map done");
        } else if (f[0] == "T") {
            int index{}; ok = g.gdxAddSetText(unhex(field(1)).c_str(), index);
        } else if (f[0] == "A") {
            ok = g.gdxAcronymAdd(unhex(field(1)).c_str(), unhex(field(2)).c_str(), std::stoi(field(3)));
        } else if (f[0] == "S") {
            auto name = unhex(field(1)), text = unhex(field(5));
            dim = std::stoi(field(2)); int type = std::stoi(field(3)), user = std::stoi(field(4));
            if (dim < 0 || dim > 20) throw std::runtime_error("invalid recipe dimension");
            mode = field(6);
            if (mode == "str") ok = g.gdxDataWriteStrStart(name.c_str(), text.c_str(), dim, type, user);
            else if (mode == "map") ok = g.gdxDataWriteMapStart(name.c_str(), text.c_str(), dim, type, user);
            else if (mode == "bound") {
                int lo[20]{}, hi[20]{};
                auto l = split(field(7), ','), h = split(field(8), ',');
                if (static_cast<int>(l.size()) != dim || static_cast<int>(h.size()) != dim)
                    throw std::runtime_error("recipe bounds dimension mismatch");
                for (int d = 0; d < dim; ++d) { lo[d] = std::stoi(l[d]); hi[d] = std::stoi(h[d]); }
                ok = g.gdxDataWriteRawStartKeyBounds(name.c_str(), text.c_str(), dim, type, user, lo, hi);
            } else if (mode == "raw") ok = g.gdxDataWriteRawStart(name.c_str(), text.c_str(), dim, type, user);
            else throw std::runtime_error("invalid recipe write mode");
            if (ok) ++symbol;
        } else if (f[0] == "D" || f[0] == "X") {
            std::vector<std::string> labels; std::vector<const char *> ptrs;
            if (f.size() != static_cast<std::size_t>(dim + 1)) throw std::runtime_error("domain dimension mismatch");
            for (int d = 0; d < dim; ++d) labels.push_back(unhex(field(d + 1)));
            for (auto &s : labels) ptrs.push_back(s.c_str());
            ok = f[0] == "D" ? g.gdxSymbolSetDomain(ptrs.data()) : g.gdxSymbolSetDomainX(symbol, ptrs.data());
        } else if (f[0] == "C") ok = g.gdxSymbolAddComment(symbol, unhex(field(1)).c_str());
        else if (f[0] == "K") {
            int keys[20]{}; double vals[5]{};
            auto k = field(1).empty() ? std::vector<std::string>{} : split(field(1), ',');
            auto v = split(field(2), ',');
            if (static_cast<int>(k.size()) != dim || v.empty() || v.size() > 5)
                throw std::runtime_error("recipe record arity mismatch");
            for (std::size_t n = 0; n < v.size(); ++n) vals[n] = recipe_value(g, v[n]);
            if (mode == "str") {
                std::vector<std::string> labels; std::vector<const char *> ptrs;
                for (auto &s : k) labels.push_back(unhex(s));
                for (auto &s : labels) ptrs.push_back(s.c_str());
                ok = g.gdxDataWriteStr(ptrs.data(), vals);
            } else {
                for (int d = 0; d < dim; ++d) keys[d] = std::stoi(k[d]);
                ok = mode == "map" ? g.gdxDataWriteMap(keys, vals) : g.gdxDataWriteRaw(keys, vals);
            }
        } else if (f[0] == "END") ok = g.gdxDataWriteDone();
        else if (f[0] == "ALIAS") {
            ok = g.gdxAddAlias(unhex(field(1)).c_str(), unhex(field(2)).c_str());
            if (ok) ++symbol;
        } else if (f[0] == "APPEND") {
            if (g.gdxClose()) throw std::runtime_error("close before append");
            ok = g.gdxOpenAppend(path, "gdxrs-append", error);
            g.gdxAutoConvert(0);
        } else throw std::runtime_error("unknown recipe directive");
        if (negative) {
            if (ok) throw std::runtime_error("expected operation failure at line " + std::to_string(line_no));
            std::cout << "REJECT\t" << f[0] << '\t' << g.gdxGetLastError() << '\t' << g.gdxDataErrorCount() << '\n';
        } else require(g, ok, ("recipe line " + std::to_string(line_no)).c_str());
    }
    if (!in.eof()) throw std::runtime_error("recipe input failure");
    if (g.gdxClose()) throw std::runtime_error("recipe close failed");
}
