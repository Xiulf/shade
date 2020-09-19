#![feature(extern_types)]
#![feature(hash_raw_entry)]

pub mod constraint;
pub mod layout;
pub mod list;
mod sharded;
pub mod subst;
pub mod tcx;
pub mod ty;

pub fn with_tcx<'a, T>(
    reporter: &diagnostics::Reporter,
    package: &hir::Package,
    module_structure: &hir::resolve::ModuleStructure,
    target: &target_lexicon::Triple,
    typemaps: impl Iterator<Item = &'a std::path::Path>,
    f: impl FnOnce(tcx::Tcx) -> T,
) -> T {
    let arena = bumpalo::Bump::new();
    let tcx = tcx::Tcx::new(reporter, &arena, &target, package, module_structure);

    for path in typemaps {
        tcx.load_type_map(path);
    }

    for (id, _) in &package.items {
        tcx.type_of(id);
    }

    tcx.unify();

    if !reporter.has_errors() {
        tcx.verify();
    }

    reporter.report(true);

    // for (id, ty) in tcx.types.borrow().iter() {
    //     println!("{}: {}", id, ty);
    // }

    f(tcx)
}
