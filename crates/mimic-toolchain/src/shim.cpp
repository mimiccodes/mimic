// C entry points for the linker and C compiler embedded in `mimic`.
//
// LLD is linked from its static libraries; Clang's driver is compiled from
// clang/tools/driver of the same LLVM release (driver.cpp, cc1_main.cpp,
// cc1as_main.cpp), so `mimic cc` behaves exactly like `clang`.

#include "lld/Common/Driver.h"
#include "llvm/ADT/ArrayRef.h"
#include "llvm/Object/Archive.h"
#include "llvm/Object/ArchiveWriter.h"
#include "llvm/Support/Error.h"
#include "llvm/Support/Path.h"
#include <cstring>
#include "llvm/Support/LLVMDriver.h"
#include "llvm/Support/raw_ostream.h"

#include <vector>

LLD_HAS_DRIVER(elf)
LLD_HAS_DRIVER(macho)
LLD_HAS_DRIVER(coff)
LLD_HAS_DRIVER(mingw)
LLD_HAS_DRIVER(wasm)

// argv[0] selects the flavour, as for the real tools:
// ld.lld (ELF), ld64.lld (Mach-O), lld-link (COFF), wasm-ld (WebAssembly).
extern "C" int mimic_lld_main(int argc, const char **argv) {
    std::vector<const char *> args(argv, argv + argc);
    lld::Result r = lld::lldMain(args, llvm::outs(), llvm::errs(), LLD_ALL_DRIVERS);
    llvm::outs().flush();
    llvm::errs().flush();
    return r.retCode;
}

int clang_main(int argc, char **argv, const llvm::ToolContext &);

// `mimic cc …`: the clang driver. When it needs to re-execute itself (it
// rarely does: cc1 runs in-process), it runs `<mimic> cc -cc1 …`.
extern "C" int mimic_clang_main(int argc, char **argv, const char *self_path) {
    llvm::ToolContext ctx;
    ctx.Path = self_path;
    ctx.PrependArg = "cc";
    ctx.NeedsPrependArg = true;
    return clang_main(argc, argv, ctx);
}

// Write a static library with a symbol index (what `ar rcs` + `ranlib` do).
// kind: 0 = GNU (ELF, WASM), 1 = Darwin (Mach-O), 2 = COFF (Windows).
// On failure returns 1 and copies the message into err.
extern "C" int mimic_write_archive(const char *out, int kind, int n, const char **members, char *err, size_t errlen) {
    auto fail = [&](llvm::Error e) {
        std::string m = llvm::toString(std::move(e));
        std::strncpy(err, m.c_str(), errlen - 1);
        err[errlen - 1] = 0;
        return 1;
    };
    std::vector<llvm::NewArchiveMember> ms;
    for (int i = 0; i < n; i++) {
        auto m = llvm::NewArchiveMember::getFile(members[i], /*Deterministic=*/true);
        if (!m)
            return fail(m.takeError());
        m->MemberName = llvm::sys::path::filename(members[i]);
        ms.push_back(std::move(*m));
    }
    llvm::object::Archive::Kind k = kind == 1 ? llvm::object::Archive::K_DARWIN
                                    : kind == 2 ? llvm::object::Archive::K_COFF
                                                : llvm::object::Archive::K_GNU;
    if (llvm::Error e = llvm::writeArchive(out, ms, llvm::SymtabWritingMode::NormalSymtab, k,
                                           /*Deterministic=*/true, /*Thin=*/false))
        return fail(std::move(e));
    return 0;
}

#include "llvm/MC/MCSubtargetInfo.h"
#include "llvm/MC/TargetRegistry.h"
#include "llvm/Support/TargetSelect.h"
#include "llvm/TargetParser/Triple.h"

// List the CPU models (what = 0) or features (what = 1) LLVM knows for a triple.
extern "C" int mimic_list_cpus(const char *triple, int what, void (*cb)(const char *, const char *, void *), void *ud) {
    llvm::InitializeAllTargetInfos();
    llvm::InitializeAllTargets();
    llvm::InitializeAllTargetMCs();
    std::string err;
    llvm::Triple tt(triple);
    const llvm::Target *t = llvm::TargetRegistry::lookupTarget(tt, err);
    if (!t)
        return 1;
    std::unique_ptr<llvm::MCSubtargetInfo> sti(t->createMCSubtargetInfo(tt, "", ""));
    if (!sti)
        return 1;
    if (what == 0) {
        for (const auto &d : sti->getAllProcessorDescriptions())
            cb(d.key(), "", ud);
    } else {
        for (const auto &f : sti->getAllProcessorFeatures())
            cb(f.key(), f.desc(), ud);
    }
    return 0;
}

// ---- target introspection: everything comes from the linked LLVM ----------

// kind: 0 = architectures, 1 = vendors, 2 = operating systems,
// 3 = environments (ABIs), 4 = code generators (name, description).
extern "C" void mimic_llvm_names(int kind, void (*cb)(const char *, const char *, void *), void *ud) {
    using llvm::Triple;
    std::string a, b;
    switch (kind) {
    case 0:
        for (int i = 1; i <= Triple::LastArchType; i++) {
            a = Triple::getArchTypeName((Triple::ArchType)i).str();
            cb(a.c_str(), "", ud);
        }
        break;
    case 1:
        for (int i = 1; i <= Triple::LastVendorType; i++) {
            a = Triple::getVendorTypeName((Triple::VendorType)i).str();
            cb(a.c_str(), "", ud);
        }
        break;
    case 2:
        for (int i = 1; i <= Triple::LastOSType; i++) {
            a = Triple::getOSTypeName((Triple::OSType)i).str();
            cb(a.c_str(), "", ud);
        }
        break;
    case 3:
        for (int i = 1; i <= Triple::LastEnvironmentType; i++) {
            a = Triple::getEnvironmentTypeName((Triple::EnvironmentType)i).str();
            cb(a.c_str(), "", ud);
        }
        break;
    default:
        llvm::InitializeAllTargetInfos();
        for (const auto &t : llvm::TargetRegistry::targets()) {
            a = t.getName();
            b = t.getShortDescription();
            cb(a.c_str(), b.c_str(), ud);
        }
    }
}

// Normalise a triple with LLVM's own parser and report what it understood.
// out gets "<normalized>\t<arch>\t<vendor>\t<os>\t<env>\t<backend>" where
// backend is the code generator's name, or empty when there is none.
extern "C" void mimic_llvm_triple(const char *in, char *out, size_t outlen) {
    using llvm::Triple;
    llvm::InitializeAllTargetInfos();
    Triple t(Triple::normalize(in));
    std::string err;
    const llvm::Target *tg = t.getArch() == Triple::UnknownArch ? nullptr : llvm::TargetRegistry::lookupTarget(t, err);
    std::string s = t.str() + "\t" + Triple::getArchTypeName(t.getArch()).str() + "\t" +
                    (t.getVendor() == Triple::UnknownVendor ? std::string() : Triple::getVendorTypeName(t.getVendor()).str()) + "\t" +
                    (t.getOS() == Triple::UnknownOS ? std::string() : Triple::getOSTypeName(t.getOS()).str()) + "\t" +
                    (t.getEnvironment() == Triple::UnknownEnvironment ? std::string() : Triple::getEnvironmentTypeName(t.getEnvironment()).str()) + "\t" +
                    (tg ? tg->getName() : "");
    std::strncpy(out, s.c_str(), outlen - 1);
    out[outlen - 1] = 0;
}
