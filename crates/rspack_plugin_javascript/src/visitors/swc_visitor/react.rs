use std::{path::Path, sync::Arc};

use once_cell::sync::Lazy;
use rspack_core::{contextify, ModuleType, ReactOptions};
use swc_core::common::{comments::SingleThreadedComments, Mark, SourceMap};
use swc_core::ecma::ast::{CallExpr, Callee, Expr, Module, ModuleItem, Program, Script, Stmt};
use swc_core::ecma::transforms::react::RefreshOptions;
use swc_core::ecma::transforms::react::{react as swc_react, Options};
use swc_core::ecma::visit::{Fold, Visit, VisitWith};

use crate::ast::parse_js_code;

pub fn react<'a>(
  top_level_mark: Mark,
  comments: Option<&'a SingleThreadedComments>,
  cm: &Arc<SourceMap>,
  options: &ReactOptions,
) -> impl Fold + 'a {
  swc_react(
    cm.clone(),
    comments,
    Options {
      refresh: options.refresh.and_then(|dev| {
        if dev {
          Some(RefreshOptions::default())
        } else {
          None
        }
      }),
      runtime: options.runtime,
      import_source: options.import_source.clone(),
      pragma: options.pragma.clone(),
      pragma_frag: options.pragma_frag.clone(),
      throw_if_namespace: options.throw_if_namespace,
      development: options.development,
      use_builtins: options.use_builtins,
      use_spread: options.use_spread,
      ..Default::default()
    },
    top_level_mark,
  )
}

pub fn fold_react_refresh(context: &Path, uri: &str) -> impl Fold {
  ReactHmrFolder {
    id: contextify(context, uri),
  }
}

pub struct FoundReactRefreshVisitor {
  pub is_refresh_boundary: bool,
}

impl Visit for FoundReactRefreshVisitor {
  fn visit_call_expr(&mut self, call_expr: &CallExpr) {
    if let Callee::Expr(expr) = &call_expr.callee {
      if let Expr::Ident(ident) = &**expr {
        if "$RefreshReg$".eq(&ident.sym) {
          self.is_refresh_boundary = true;
        }
      }
    }
  }
}

// __webpack_require__.$ReactRefreshRuntime$ is injected by the react-refresh additional entry
static HMR_HEADER: &str = r#"var RefreshRuntime = __webpack_require__.m.$ReactRefreshRuntime$;
var prevRefreshReg;
var prevRefreshSig;
prevRefreshReg = globalThis.$RefreshReg$;
prevRefreshSig = globalThis.$RefreshSig$;
globalThis.$RefreshReg$ = (type, id) => {
  RefreshRuntime.register(type, "__SOURCE__" + "_" + id);
};
globalThis.$RefreshSig$ = RefreshRuntime.createSignatureFunctionForTransform;"#;

static HMR_FOOTER: &str = r#"var RefreshRuntime = __webpack_require__.m.$ReactRefreshRuntime$;
globalThis.$RefreshReg$ = prevRefreshReg;
globalThis.$RefreshSig$ = prevRefreshSig;
module.hot.accept();
RefreshRuntime.queueUpdate();
"#;

static HMR_FOOTER_AST: Lazy<Script> = Lazy::new(|| {
  parse_js_code(HMR_FOOTER.to_string(), &ModuleType::Js)
    .expect("TODO:")
    .expect_script()
});

pub struct ReactHmrFolder {
  pub id: String,
}

impl ReactHmrFolder {
  fn pseudo_fold_program(&mut self, program: Program) -> Program {
    let mut f = FoundReactRefreshVisitor {
      is_refresh_boundary: false,
    };

    program.visit_with(&mut f);
    if !f.is_refresh_boundary {
      return program;
    }
    // TODO: cache the ast
    let hmr_header_ast = parse_js_code(
      HMR_HEADER.replace("__SOURCE__", self.id.as_str()),
      &ModuleType::Js,
    )
    .expect("Should never fail to parse HMR header code")
    .expect_script();

    match program {
      Program::Module(module) => {
        let mut body: Vec<_> = to_module_body(hmr_header_ast.body);

        body.extend(module.body);
        body.extend(to_module_body(HMR_FOOTER_AST.body.clone()));

        Program::Module(Module { body, ..module })
      }
      Program::Script(script) => {
        let mut body = hmr_header_ast.body;

        body.extend(script.body);
        body.extend(HMR_FOOTER_AST.body.clone());

        Program::Script(Script { body, ..script })
      }
    }
  }
}

impl Fold for ReactHmrFolder {
  /* TODO:Current version of SWC, fold_program cannot be used in the chain, remove this when fixed */
  fn fold_module(&mut self, module: Module) -> Module {
    self
      .pseudo_fold_program(Program::Module(module))
      .expect_module()
  }

  /* TODO:Current version of SWC, fold_program cannot be used in the chain, remove this when fixed */
  fn fold_script(&mut self, script: Script) -> Script {
    self
      .pseudo_fold_program(Program::Script(script))
      .expect_script()
  }
}

fn to_module_body(script_body: Vec<Stmt>) -> Vec<ModuleItem> {
  script_body.into_iter().map(ModuleItem::Stmt).collect()
}
