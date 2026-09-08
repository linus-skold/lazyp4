// C++ side of the P4API bridge. Declarations only — see src/shim.cc.
#pragma once

#include <memory>

#include "rust/cxx.h"

namespace lazyp4 {

// Defined by the generated bridge header; used by value only in definitions.
struct RunOutput;

class P4Client {
   public:
    P4Client();
    ~P4Client();

    P4Client(const P4Client &) = delete;
    P4Client &operator=(const P4Client &) = delete;

    void set_port(rust::Str value);
    void set_user(rust::Str value);
    void set_client(rust::Str value);
    void set_password(rust::Str value);
    void set_charset(rust::Str value);
    void set_cwd(rust::Str value);
    void set_prog(rust::Str value);
    void set_version(rust::Str value);
    void set_tagged(bool on);

    void connect();
    void disconnect();
    bool dropped();

    RunOutput run(rust::Str cmd, const rust::Vec<rust::String> &args,
                  rust::Str input);

   private:
    struct Impl;
    std::unique_ptr<Impl> impl_;
};

std::unique_ptr<P4Client> new_client();

}  // namespace lazyp4
