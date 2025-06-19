extern crate rustc_driver;
extern crate rustc_hir;
extern crate rustc_interface;
extern crate rustc_middle;
extern crate rustc_span;


use rustc_driver::Callbacks;
use rustc_hir::{ForeignItemKind, ItemKind};
use rustc_interface::Queries;
use rustc_middle::ty::TyCtxt;


// struct storing ffi function namesstruct FfiExtractor {
pub struct FfiExtractor {
    pub ffi_functions: Vec<String>,
}


impl FfiExtractor {
    pub fn new() -> Self {
        Self { ffi_functions: Vec::new() }
    }

    fn scan_ffi_functions<'tcx>(&mut self, tcx: TyCtxt<'tcx>) {
        let hir = tcx.hir();
        
        for item_id in hir.items() {
            let item = hir.item(item_id);
            if let ItemKind::ForeignMod { items, .. } = &item.kind {
                for foreign_item in *items {
                    if let ForeignItemKind::Fn(_, _, _) = &hir.foreign_item(foreign_item.id).kind {
                        let fn_name = hir.foreign_item(foreign_item.id).ident.to_string();
                        self.ffi_functions.push(fn_name);
                    }
                }
            }
        }
    }
}

impl Callbacks for FfiExtractor {
    fn after_analysis<'tcx>(
        &mut self,
        _compiler: &rustc_interface::interface::Compiler,
        queries: &'tcx Queries<'tcx>,
    ) -> rustc_driver::Compilation {
        queries.global_ctxt().unwrap().enter(|tcx| {
            self.scan_ffi_functions(tcx);
        });
        rustc_driver::Compilation::Stop
    }
}



