#include "p4-sys/include/shim.h"

#include "p4-sys/src/lib.rs.h"

#include <clientapi.h>
#include <p4libs.h>

#include <cstring>
#include <mutex>
#include <stdexcept>
#include <string>
#include <vector>

namespace lazyp4 {
namespace {

std::string to_std(rust::Str s) { return std::string(s.data(), s.size()); }

rust::String to_rust(const StrPtr &s) {
    // P4 content is only UTF-8 when P4CHARSET says so; never trust it.
    return rust::String::lossy(s.Text(), s.Length());
}

std::string format(const Error &e) {
    StrBuf buf;
    e.Fmt(&buf, EF_PLAIN);
    return std::string(buf.Text(), buf.Length());
}

// The P4API needs one process-wide init before any ClientApi is used.
// There is no matching Finalize call: it must not run while another client is
// still alive, and process exit reclaims everything anyway.
void init_libraries() {
    static std::once_flag once;
    static std::string failure;
    std::call_once(once, [] {
        Error e;
        P4Libraries::Initialize(P4LIBRARIES_INIT_ALL, &e);
        if (e.Test()) failure = format(e);
    });
    if (!failure.empty()) throw std::runtime_error(failure);
}

// Buffers one command's ClientUser callbacks into a flat RunOutput.
class Collector : public ClientUser {
   public:
    explicit Collector(std::string input) : input_(std::move(input)) {}

    RunOutput take() { return std::move(out_); }

    // Feeds `p4 <spec> -i` and anything else reading the client's stdin.
    void InputData(StrBuf *buf, Error *) override { buf->Set(input_.c_str()); }

    void OutputStat(StrDict *varList) override {
        TaggedRecord rec;
        StrRef var, val;
        for (int i = 0; varList->GetVar(i, var, val); ++i) {
            // `func` is RPC dispatch metadata, not part of the record.
            if (std::strcmp(var.Text(), "func") == 0) continue;
            TaggedField field;
            field.key = to_rust(var);
            field.value = to_rust(val);
            rec.fields.push_back(std::move(field));
        }
        out_.records.push_back(std::move(rec));
    }

    void OutputInfo(char, const char *data) override {
        out_.info.push_back(rust::String::lossy(data));
    }

    void OutputText(const char *data, int length) override {
        append(data, length);
    }

    void OutputBinary(const char *data, int length) override {
        append(data, length);
    }

    void OutputError(const char *data) override {
        P4Message msg;
        msg.severity = E_FAILED;
        msg.generic = 0;
        msg.text = rust::String::lossy(data);
        out_.messages.push_back(std::move(msg));
    }

    // Message() supersedes OutputInfo/HandleError on 2002.1+ servers. Both are
    // overridden so the base class never routes one into the other and records
    // it twice.
    void Message(Error *err) override { record(err); }
    void HandleError(Error *err) override { record(err); }

    // Never block on a terminal: the answer is whatever the caller supplied.
    void Prompt(const StrPtr &, StrBuf &rsp, int, Error *) override {
        rsp.Set(input_.c_str());
    }
    void Prompt(const StrPtr &, StrBuf &rsp, int, int, Error *) override {
        rsp.Set(input_.c_str());
    }
    void Prompt(Error *, StrBuf &rsp, int, Error *) override {
        rsp.Set(input_.c_str());
    }
    void Prompt(Error *, StrBuf &rsp, int, int, Error *) override {
        rsp.Set(input_.c_str());
    }

   private:
    void append(const char *data, int length) {
        out_.text.reserve(out_.text.size() + length);
        for (int i = 0; i < length; ++i)
            out_.text.push_back(static_cast<uint8_t>(data[i]));
    }

    void record(Error *err) {
        if (!err || err->GetSeverity() == E_EMPTY) return;
        if (err->GetSeverity() == E_INFO) {
            out_.info.push_back(rust::String::lossy(format(*err)));
            return;
        }
        P4Message msg;
        msg.severity = err->GetSeverity();
        msg.generic = err->GetGeneric();
        msg.text = rust::String::lossy(format(*err));
        out_.messages.push_back(std::move(msg));
    }

    std::string input_;
    RunOutput out_;
};

}  // namespace

struct P4Client::Impl {
    ClientApi client;
    bool connected = false;
};

P4Client::P4Client() : impl_(std::make_unique<Impl>()) { init_libraries(); }

P4Client::~P4Client() {
    if (impl_ && impl_->connected) {
        Error e;
        impl_->client.Final(&e);
    }
}

void P4Client::set_port(rust::Str v) { impl_->client.SetPort(to_std(v).c_str()); }
void P4Client::set_user(rust::Str v) { impl_->client.SetUser(to_std(v).c_str()); }
void P4Client::set_client(rust::Str v) { impl_->client.SetClient(to_std(v).c_str()); }
void P4Client::set_password(rust::Str v) { impl_->client.SetPassword(to_std(v).c_str()); }
void P4Client::set_charset(rust::Str v) { impl_->client.SetCharset(to_std(v).c_str()); }
void P4Client::set_cwd(rust::Str v) { impl_->client.SetCwd(to_std(v).c_str()); }
void P4Client::set_prog(rust::Str v) { impl_->client.SetProg(to_std(v).c_str()); }
void P4Client::set_version(rust::Str v) { impl_->client.SetVersion(to_std(v).c_str()); }

void P4Client::set_tagged(bool on) {
    if (on) impl_->client.SetProtocol("tag", "");
}

void P4Client::connect() {
    if (impl_->connected) return;
    Error e;
    impl_->client.Init(&e);
    if (e.Test()) throw std::runtime_error(format(e));
    impl_->connected = true;
}

void P4Client::disconnect() {
    if (!impl_->connected) return;
    Error e;
    impl_->client.Final(&e);
    impl_->connected = false;
    if (e.Test()) throw std::runtime_error(format(e));
}

bool P4Client::dropped() { return impl_->client.Dropped() != 0; }

RunOutput P4Client::run(rust::Str cmd, const rust::Vec<rust::String> &args,
                        rust::Str input) {
    if (!impl_->connected) throw std::runtime_error("not connected");

    std::vector<std::string> owned;
    owned.reserve(args.size());
    for (const auto &a : args) owned.emplace_back(a.data(), a.size());

    std::vector<char *> argv;
    argv.reserve(owned.size());
    for (auto &a : owned) argv.push_back(a.data());

    Collector ui(to_std(input));
    impl_->client.SetArgv(static_cast<int>(argv.size()),
                          argv.empty() ? nullptr : argv.data());
    impl_->client.Run(to_std(cmd).c_str(), &ui);

    if (impl_->client.Dropped()) {
        impl_->connected = false;
        throw std::runtime_error("connection to the Perforce server was lost");
    }
    return ui.take();
}

std::unique_ptr<P4Client> new_client() { return std::make_unique<P4Client>(); }

}  // namespace lazyp4
