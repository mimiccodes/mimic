//! Debug information (DWARF), for debug builds (G4).
//!
//! Every function the program defines gets a subprogram: the program's own
//! functions under their Mimic names (`main::fib`), at their line; what the
//! compiler makes (lambdas, kernels, thunks) as artificial ones in the same
//! file. Each expression sets the source location (line and column) of the
//! code made for it, so a debugger steps through `.mim` lines and a crash
//! shows them. Parameters and `let`s of numbers and truth values are
//! described as variables (`frame variable` shows them); other values are
//! handles, shown as pointers.

use super::*;
use inkwell::debug_info::{AsDIScope, DIFile, DIFlags, DIFlagsConstants, DISubprogram, DIType, DWARFEmissionKind, DWARFSourceLanguage, DebugInfoBuilder};

pub(crate) struct Dbg<'ctx> {
    b: DebugInfoBuilder<'ctx>,
    cu: inkwell::debug_info::DICompileUnit<'ctx>,
    files: HashMap<usize, DIFile<'ctx>>,
    /// the subprogram of the function being built, and its file
    cur: Option<(DISubprogram<'ctx>, DIFile<'ctx>)>,
    types: HashMap<&'static str, DIType<'ctx>>,
}

/// The parts of a path: file name and directory.
fn split(p: &Path) -> (String, String) {
    let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "main.mim".into());
    let dir = p.parent().map(|d| d.to_string_lossy().into_owned()).unwrap_or_default();
    (name, dir)
}

impl<'ctx> Dbg<'ctx> {
    pub(crate) fn new(module: &Module<'ctx>, ctx: &'ctx Context, main_file: &Path, optimized: bool) -> Self {
        let (name, dir) = split(main_file);
        let (b, cu) = module.create_debug_info_builder(
            true,
            DWARFSourceLanguage::C,
            &name,
            &dir,
            "mimic",
            optimized,
            "",
            0,
            "",
            DWARFEmissionKind::Full,
            0,
            false,
            false,
            "",
            "",
        );
        let i32t = ctx.i32_type();
        module.add_basic_value_flag("Debug Info Version", inkwell::module::FlagBehavior::Warning, i32t.const_int(3, false));
        module.add_basic_value_flag("Dwarf Version", inkwell::module::FlagBehavior::Warning, i32t.const_int(4, false));
        let mut types = HashMap::new();
        // DW_ATE_boolean 2, float 4, signed 5, address 1
        for (n, bits, enc) in [("i64", 64, 5), ("i32", 32, 5), ("f64", 64, 4), ("f32", 32, 4), ("bool", 8, 2)] {
            if let Ok(t) = b.create_basic_type(n, bits, enc, DIFlags::ZERO) {
                types.insert(n, t.as_type());
            }
        }
        // a handle: `value`, a pointer to the runtime's block (header and
        // elements); `mimic lldb` shows what it holds by that name
        if let Ok(block) = b.create_basic_type("mim_val", 8, 8, DIFlags::ZERO) {
            let ptr = b.create_pointer_type("mim_val *", block.as_type(), 64, 64, inkwell::AddressSpace::default());
            let td = b.create_typedef(ptr.as_type(), "value", cu.get_file(), 0, cu.as_debug_info_scope(), 64);
            types.insert("value", td.as_type());
        }
        Dbg { b, cu, files: HashMap::new(), cur: None, types }
    }

    pub(crate) fn finalize(&self) {
        self.b.finalize();
    }
}

impl<'ctx> Cg<'ctx, '_> {
    fn dbg_file(&mut self, m: ModId) -> Option<DIFile<'ctx>> {
        let fi = self.p.mods[m].file?;
        let d = self.dbg.as_mut()?;
        if let Some(f) = d.files.get(&fi) {
            return Some(*f);
        }
        let (name, dir) = split(&self.p.files[fi].path);
        let f = d.b.create_file(&name, &dir);
        d.files.insert(fi, f);
        Some(f)
    }

    /// Start describing `fv` (being built now): a subprogram named `name` at
    /// `line` of module `m`'s file (or of the current function's file).
    pub(crate) fn dbg_enter(&mut self, fv: FunctionValue<'ctx>, name: &str, m: Option<ModId>, line: u32, artificial: bool) {
        if self.dbg.is_none() {
            return;
        }
        let file = match m.and_then(|m| self.dbg_file(m)) {
            Some(f) => f,
            None => match self.dbg.as_ref().and_then(|d| d.cur.map(|(_, f)| f)) {
                Some(f) => f,
                None => {
                    self.b.unset_current_debug_location();
                    return;
                }
            },
        };
        let optimized = self.opt_level >= 2;
        let d = self.dbg.as_mut().unwrap();
        let ty = d.b.create_subroutine_type(file, None, &[], DIFlags::ZERO);
        // (no linkage name: debuggers then show the Mimic name, `main::area`)
        let _ = fv.get_name();
        let flags = if artificial { DIFlags::ARTIFICIAL } else { DIFlags::ZERO };
        let sp = d.b.create_function(d.cu.as_debug_info_scope(), name, None, file, line, ty, true, true, line, flags, optimized);
        fv.set_subprogram(sp);
        d.cur = Some((sp, file));
        let loc = d.b.create_debug_location(self.ctx, line, 0, sp.as_debug_info_scope(), None);
        self.b.set_current_debug_location(loc);
    }

    /// Code made from now on is at `span` (of the current function's file).
    pub(crate) fn dbg_at(&self, span: mimic_syntax::Span) {
        let Some(d) = self.dbg.as_ref() else { return };
        let Some((sp, _)) = d.cur else { return };
        if span.line == 0 {
            return;
        }
        let loc = d.b.create_debug_location(self.ctx, span.line, span.col, sp.as_debug_info_scope(), None);
        self.b.set_current_debug_location(loc);
    }

    /// No function is being described (code made now has no location).
    pub(crate) fn dbg_leave(&mut self) {
        if let Some(d) = self.dbg.as_mut() {
            d.cur = None;
            self.b.unset_current_debug_location();
        }
    }

    /// The state to come back to after building another function.
    pub(crate) fn dbg_save(&self) -> Option<(Option<(DISubprogram<'ctx>, DIFile<'ctx>)>, Option<inkwell::debug_info::DILocation<'ctx>>)> {
        let d = self.dbg.as_ref()?;
        Some((d.cur, self.b.get_current_debug_location()))
    }

    pub(crate) fn dbg_restore(&mut self, s: Option<(Option<(DISubprogram<'ctx>, DIFile<'ctx>)>, Option<inkwell::debug_info::DILocation<'ctx>>)>) {
        let Some((cur, loc)) = s else { return };
        if let Some(d) = self.dbg.as_mut() {
            d.cur = cur;
        }
        match loc {
            Some(l) => self.b.set_current_debug_location(l),
            None => self.b.unset_current_debug_location(),
        }
    }

    /// Describe the variable `name` of type `t` kept at `slot` (a parameter
    /// when `arg` is its 1-based position).
    pub(crate) fn dbg_var(&self, name: &str, slot: PointerValue<'ctx>, t: Ty, line: u32, arg: Option<u32>) {
        let Some(d) = self.dbg.as_ref() else { return };
        let Some((sp, file)) = d.cur else { return };
        let key = |t: Ty| match t {
            Ty::I64 => Some("i64"),
            Ty::I32 => Some("i32"),
            Ty::F64 => Some("f64"),
            Ty::F32 => Some("f32"),
            Ty::Bool => Some("bool"),
            _ => None,
        };
        let ty = match (key(t), fixed(t)) {
            (Some(k), _) => d.types.get(k).copied(),
            // a list of known length kept in registers: an array of its elements
            (None, Some((el, n))) => {
                let Some(et) = key(Ty::of_el(el)).and_then(|k| d.types.get(k).copied()) else { return };
                let bits = if el == El::Bool { 8 } else { 64 };
                Some(d.b.create_array_type(et, bits * n as u64, 64, &[0..n as i64]).as_type())
            }
            _ if self.repr(t).is_pointer_type() => d.types.get("value").copied(),
            _ => None,
        };
        let Some(ty) = ty else { return };
        let Some(bb) = self.b.get_insert_block() else { return };
        let scope = sp.as_debug_info_scope();
        let var = match arg {
            Some(n) => d.b.create_parameter_variable(scope, name, n, file, line, ty, true, DIFlags::ZERO),
            None => d.b.create_auto_variable(scope, name, file, line, ty, true, DIFlags::ZERO, 0),
        };
        let loc = d.b.create_debug_location(self.ctx, line.max(1), 0, scope, None);
        d.b.insert_declare_at_end(slot, Some(var), Some(d.b.create_expression(vec![])), loc, bb);
    }
}
