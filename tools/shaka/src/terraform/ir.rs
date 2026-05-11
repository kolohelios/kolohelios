//! Internal representation of a Terraform module. Populated from the
//! JSON output of `cue export` (see `cue.rs`) and consumed by the HCL
//! emitter (see `emit.rs`).

use serde_json::{Map, Value as Json};
use snafu::Snafu;

#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
pub enum IrError {
    #[snafu(display("at {path}: expected {expected}, got {got}"))]
    Type {
        path: String,
        expected: &'static str,
        got: String,
    },
    #[snafu(display("at {path}: missing required field `{field}`"))]
    MissingField { path: String, field: &'static str },
    #[snafu(display("at {path}: $ref expression is not a valid traversal: {expr}"))]
    InvalidRef { path: String, expr: String },
}

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Bool(bool),
    Number(serde_json::Number),
    String(String),
    Ref(String),
    List(Vec<Expr>),
    Object(Vec<(String, Expr)>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Variable {
    pub name: String,
    pub description: Option<String>,
    pub ty: String,
    pub default: Option<Expr>,
    pub sensitive: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Output {
    pub name: String,
    pub description: Option<String>,
    pub value: Expr,
    pub sensitive: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct NestedBlock {
    pub ty: String,
    pub attributes: Vec<(String, Expr)>,
    pub blocks: Vec<NestedBlock>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Resource {
    pub ty: String,
    pub name: String,
    pub attributes: Vec<(String, Expr)>,
    pub blocks: Vec<NestedBlock>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Provider {
    pub name: String,
    pub attributes: Vec<(String, Expr)>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RequiredProvider {
    pub local_name: String,
    pub source: String,
    pub version: String,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct TerraformBlock {
    pub required_version: Option<String>,
    pub required_providers: Vec<RequiredProvider>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Module {
    pub terraform: Option<TerraformBlock>,
    pub providers: Vec<Provider>,
    pub variables: Vec<Variable>,
    pub resources: Vec<Resource>,
    pub outputs: Vec<Output>,
}

pub fn from_json(value: &Json) -> Result<Module, IrError> {
    let obj = as_object(value, "")?;
    let mut module = Module::default();

    if let Some(tf) = obj.get("terraform") {
        module.terraform = Some(parse_terraform_block(tf, "terraform")?);
    }
    if let Some(arr) = obj.get("providers") {
        for (i, item) in as_array(arr, "providers")?.iter().enumerate() {
            module
                .providers
                .push(parse_provider(item, &format!("providers[{i}]"))?);
        }
    }
    if let Some(arr) = obj.get("variables") {
        for (i, item) in as_array(arr, "variables")?.iter().enumerate() {
            module
                .variables
                .push(parse_variable(item, &format!("variables[{i}]"))?);
        }
    }
    if let Some(arr) = obj.get("resources") {
        for (i, item) in as_array(arr, "resources")?.iter().enumerate() {
            module
                .resources
                .push(parse_resource(item, &format!("resources[{i}]"))?);
        }
    }
    if let Some(arr) = obj.get("outputs") {
        for (i, item) in as_array(arr, "outputs")?.iter().enumerate() {
            module
                .outputs
                .push(parse_output(item, &format!("outputs[{i}]"))?);
        }
    }

    Ok(module)
}

fn parse_terraform_block(value: &Json, path: &str) -> Result<TerraformBlock, IrError> {
    let obj = as_object(value, path)?;
    let required_version = obj
        .get("required_version")
        .map(|v| as_string(v, &format!("{path}.required_version")))
        .transpose()?;
    let mut required_providers = Vec::new();
    if let Some(rp) = obj.get("required_providers") {
        let rp_obj = as_object(rp, &format!("{path}.required_providers"))?;
        let mut entries: Vec<(&String, &Json)> = rp_obj.iter().collect();
        entries.sort_by(|a, b| a.0.cmp(b.0));
        for (name, val) in entries {
            let inner = as_object(val, &format!("{path}.required_providers.{name}"))?;
            let source = require_string(
                inner,
                &format!("{path}.required_providers.{name}"),
                "source",
            )?;
            let version = require_string(
                inner,
                &format!("{path}.required_providers.{name}"),
                "version",
            )?;
            required_providers.push(RequiredProvider {
                local_name: name.clone(),
                source,
                version,
            });
        }
    }
    Ok(TerraformBlock {
        required_version,
        required_providers,
    })
}

fn parse_provider(value: &Json, path: &str) -> Result<Provider, IrError> {
    let obj = as_object(value, path)?;
    let name = require_string(obj, path, "name")?;
    let attributes = match obj.get("attributes") {
        Some(a) => parse_attributes(a, &format!("{path}.attributes"))?,
        None => Vec::new(),
    };
    Ok(Provider { name, attributes })
}

fn parse_variable(value: &Json, path: &str) -> Result<Variable, IrError> {
    let obj = as_object(value, path)?;
    let name = require_string(obj, path, "name")?;
    let description = obj
        .get("description")
        .map(|v| as_string(v, &format!("{path}.description")))
        .transpose()?;
    let ty = require_string(obj, path, "type")?;
    let default = obj
        .get("default")
        .map(|v| parse_expr(v, &format!("{path}.default")))
        .transpose()?;
    let sensitive = obj
        .get("sensitive")
        .map(|v| as_bool(v, &format!("{path}.sensitive")))
        .transpose()?
        .unwrap_or(false);
    Ok(Variable {
        name,
        description,
        ty,
        default,
        sensitive,
    })
}

fn parse_output(value: &Json, path: &str) -> Result<Output, IrError> {
    let obj = as_object(value, path)?;
    let name = require_string(obj, path, "name")?;
    let description = obj
        .get("description")
        .map(|v| as_string(v, &format!("{path}.description")))
        .transpose()?;
    let value_field = obj.get("value").ok_or_else(|| IrError::MissingField {
        path: path.to_string(),
        field: "value",
    })?;
    let v = parse_expr(value_field, &format!("{path}.value"))?;
    let sensitive = obj
        .get("sensitive")
        .map(|v| as_bool(v, &format!("{path}.sensitive")))
        .transpose()?
        .unwrap_or(false);
    Ok(Output {
        name,
        description,
        value: v,
        sensitive,
    })
}

fn parse_resource(value: &Json, path: &str) -> Result<Resource, IrError> {
    let obj = as_object(value, path)?;
    let ty = require_string(obj, path, "type")?;
    let name = require_string(obj, path, "name")?;
    let attributes = parse_attributes(
        obj.get("attributes").ok_or_else(|| IrError::MissingField {
            path: path.to_string(),
            field: "attributes",
        })?,
        &format!("{path}.attributes"),
    )?;
    let blocks = match obj.get("blocks") {
        Some(b) => {
            let arr = as_array(b, &format!("{path}.blocks"))?;
            arr.iter()
                .enumerate()
                .map(|(i, v)| parse_nested_block(v, &format!("{path}.blocks[{i}]")))
                .collect::<Result<_, _>>()?
        }
        None => Vec::new(),
    };
    Ok(Resource {
        ty,
        name,
        attributes,
        blocks,
    })
}

fn parse_nested_block(value: &Json, path: &str) -> Result<NestedBlock, IrError> {
    let obj = as_object(value, path)?;
    let ty = require_string(obj, path, "type")?;
    let attributes = match obj.get("attributes") {
        Some(a) => parse_attributes(a, &format!("{path}.attributes"))?,
        None => Vec::new(),
    };
    let blocks = match obj.get("blocks") {
        Some(b) => {
            let arr = as_array(b, &format!("{path}.blocks"))?;
            arr.iter()
                .enumerate()
                .map(|(i, v)| parse_nested_block(v, &format!("{path}.blocks[{i}]")))
                .collect::<Result<_, _>>()?
        }
        None => Vec::new(),
    };
    Ok(NestedBlock {
        ty,
        attributes,
        blocks,
    })
}

fn parse_attributes(value: &Json, path: &str) -> Result<Vec<(String, Expr)>, IrError> {
    let obj = as_object(value, path)?;
    let mut entries: Vec<(&String, &Json)> = obj.iter().collect();
    entries.sort_by(|a, b| a.0.cmp(b.0));
    entries
        .into_iter()
        .map(|(k, v)| Ok((k.clone(), parse_expr(v, &format!("{path}.{k}"))?)))
        .collect()
}

fn parse_expr(value: &Json, path: &str) -> Result<Expr, IrError> {
    match value {
        Json::Bool(b) => Ok(Expr::Bool(*b)),
        Json::Number(n) => Ok(Expr::Number(n.clone())),
        Json::String(s) => Ok(Expr::String(s.clone())),
        Json::Array(items) => {
            let exprs = items
                .iter()
                .enumerate()
                .map(|(i, v)| parse_expr(v, &format!("{path}[{i}]")))
                .collect::<Result<_, _>>()?;
            Ok(Expr::List(exprs))
        }
        Json::Object(obj) => {
            // Detect a #Ref: an object whose only field is `$ref` and
            // whose value is a string matching the traversal pattern.
            if obj.len() == 1 {
                if let Some(Json::String(expr)) = obj.get("$ref") {
                    if is_traversal(expr) {
                        return Ok(Expr::Ref(expr.clone()));
                    }
                    return Err(IrError::InvalidRef {
                        path: path.to_string(),
                        expr: expr.clone(),
                    });
                }
            }
            let mut entries: Vec<(&String, &Json)> = obj.iter().collect();
            entries.sort_by(|a, b| a.0.cmp(b.0));
            let pairs = entries
                .into_iter()
                .map(|(k, v)| Ok((k.clone(), parse_expr(v, &format!("{path}.{k}"))?)))
                .collect::<Result<_, _>>()?;
            Ok(Expr::Object(pairs))
        }
        Json::Null => Err(IrError::Type {
            path: path.to_string(),
            expected: "non-null value",
            got: "null".into(),
        }),
    }
}

fn is_traversal(s: &str) -> bool {
    let mut parts = s.split('.');
    let Some(first) = parts.next() else {
        return false;
    };
    if !is_ident(first) {
        return false;
    }
    for p in parts {
        if !is_ident(p) {
            return false;
        }
    }
    true
}

fn is_ident(s: &str) -> bool {
    let mut chars = s.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !(first.is_ascii_alphabetic() || first == '_') {
        return false;
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn as_object<'a>(value: &'a Json, path: &str) -> Result<&'a Map<String, Json>, IrError> {
    value.as_object().ok_or_else(|| IrError::Type {
        path: path.to_string(),
        expected: "object",
        got: type_of(value).into(),
    })
}

fn as_array<'a>(value: &'a Json, path: &str) -> Result<&'a Vec<Json>, IrError> {
    value.as_array().ok_or_else(|| IrError::Type {
        path: path.to_string(),
        expected: "array",
        got: type_of(value).into(),
    })
}

fn as_string(value: &Json, path: &str) -> Result<String, IrError> {
    value
        .as_str()
        .map(String::from)
        .ok_or_else(|| IrError::Type {
            path: path.to_string(),
            expected: "string",
            got: type_of(value).into(),
        })
}

fn as_bool(value: &Json, path: &str) -> Result<bool, IrError> {
    value.as_bool().ok_or_else(|| IrError::Type {
        path: path.to_string(),
        expected: "bool",
        got: type_of(value).into(),
    })
}

fn require_string(
    obj: &Map<String, Json>,
    path: &str,
    field: &'static str,
) -> Result<String, IrError> {
    let v = obj.get(field).ok_or_else(|| IrError::MissingField {
        path: path.to_string(),
        field,
    })?;
    as_string(v, &format!("{path}.{field}"))
}

fn type_of(value: &Json) -> &'static str {
    match value {
        Json::Null => "null",
        Json::Bool(_) => "bool",
        Json::Number(_) => "number",
        Json::String(_) => "string",
        Json::Array(_) => "array",
        Json::Object(_) => "object",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parse_expr_recognizes_ref() {
        let v = json!({"$ref": "var.region"});
        assert_eq!(parse_expr(&v, "").unwrap(), Expr::Ref("var.region".into()));
    }

    #[test]
    fn parse_expr_recognizes_multi_segment_ref() {
        let v = json!({"$ref": "linode_instance.devbox.id"});
        assert_eq!(
            parse_expr(&v, "").unwrap(),
            Expr::Ref("linode_instance.devbox.id".into())
        );
    }

    #[test]
    fn parse_expr_rejects_ref_with_invalid_chars() {
        let v = json!({"$ref": "var.region; rm -rf /"});
        match parse_expr(&v, "x") {
            Err(IrError::InvalidRef { .. }) => {}
            other => panic!("expected InvalidRef, got {other:?}"),
        }
    }

    #[test]
    fn parse_expr_treats_object_with_other_fields_as_object_literal() {
        let v = json!({"$ref": "var.x", "extra": 1});
        let parsed = parse_expr(&v, "").unwrap();
        match parsed {
            Expr::Object(_) => {}
            other => panic!("expected object literal, got {other:?}"),
        }
    }

    #[test]
    fn parse_attributes_sorts_by_key() {
        let v = json!({"z": 1, "a": 2, "m": 3});
        let attrs = parse_attributes(&v, "").unwrap();
        let keys: Vec<&str> = attrs.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, vec!["a", "m", "z"]);
    }

    #[test]
    fn from_json_parses_empty_module() {
        let v = json!({});
        let m = from_json(&v).unwrap();
        assert_eq!(m, Module::default());
    }

    #[test]
    fn from_json_parses_minimal_resource() {
        let v = json!({
            "resources": [
                {
                    "type": "linode_instance",
                    "name": "devbox",
                    "attributes": {"label": "devbox"}
                }
            ]
        });
        let m = from_json(&v).unwrap();
        assert_eq!(m.resources.len(), 1);
        assert_eq!(m.resources[0].ty, "linode_instance");
        assert_eq!(m.resources[0].name, "devbox");
        assert_eq!(
            m.resources[0].attributes,
            vec![("label".into(), Expr::String("devbox".into()))]
        );
    }

    #[test]
    fn from_json_parses_nested_block() {
        let v = json!({
            "resources": [
                {
                    "type": "linode_volume",
                    "name": "persist",
                    "attributes": {},
                    "blocks": [
                        {
                            "type": "lifecycle",
                            "attributes": {"prevent_destroy": true}
                        }
                    ]
                }
            ]
        });
        let m = from_json(&v).unwrap();
        assert_eq!(m.resources[0].blocks.len(), 1);
        assert_eq!(m.resources[0].blocks[0].ty, "lifecycle");
        assert_eq!(
            m.resources[0].blocks[0].attributes,
            vec![("prevent_destroy".into(), Expr::Bool(true))]
        );
    }

    #[test]
    fn from_json_parses_required_providers_sorted() {
        let v = json!({
            "terraform": {
                "required_version": ">= 1.0",
                "required_providers": {
                    "linode": {"source": "linode/linode", "version": "~> 2.0"},
                    "cloudflare": {"source": "cloudflare/cloudflare", "version": "~> 5.0"}
                }
            }
        });
        let m = from_json(&v).unwrap();
        let tf = m.terraform.unwrap();
        assert_eq!(tf.required_version.as_deref(), Some(">= 1.0"));
        let names: Vec<&str> = tf
            .required_providers
            .iter()
            .map(|p| p.local_name.as_str())
            .collect();
        assert_eq!(names, vec!["cloudflare", "linode"]);
    }

    #[test]
    fn is_traversal_accepts_dotted_identifiers() {
        assert!(is_traversal("var.region"));
        assert!(is_traversal("linode_instance.devbox.id"));
        assert!(is_traversal("module.foo.bar.baz"));
        assert!(is_traversal("_underscore"));
    }

    #[test]
    fn is_traversal_rejects_non_identifiers() {
        assert!(!is_traversal(""));
        assert!(!is_traversal("var."));
        assert!(!is_traversal(".var"));
        assert!(!is_traversal("var.region; rm -rf /"));
        assert!(!is_traversal("var.with space"));
        assert!(!is_traversal("123.foo"));
    }
}
