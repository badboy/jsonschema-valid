use std::collections::HashSet;
use std::fmt;
use std::fmt::Debug;
use std::hash::Hash;
use std::hash::Hasher;
use std::iter;

use itertools::Itertools;

use regex;

use serde_json::{Map, Value};

type Validator = fn(instance: &Value, schema: &Value, parent_schema: &Map<String, Value>) -> ValidatorResult;

#[derive(Default)]
pub struct ValidationError {
  msg: String,
  instance_path: Vec<String>,
  schema_path: Vec<String>
}

impl Debug for ValidationError {
  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
    let instance_path = self.instance_path.iter().rev().join("/");
    let schema_path = self.schema_path.iter().rev().join("/");
    write!(f, "At {} in schema {}: {}",
           instance_path,
           schema_path,
           self.msg)
  }
}

impl ValidationError {
  pub fn new(msg: &str) -> ValidationError {
    ValidationError {
      msg: String::from(msg),
      ..Default::default()
    }
  }
}

pub type ValidatorResult = Result<(), ValidationError>;

fn get_validator(key: &str) -> Option<Validator> {
  match key {
    "patternProperties" => Some(validate_patternProperties as Validator),
    "propertyNames" => Some(validate_propertyNames as Validator),
    "additionalProperties" => Some(validate_additionalProperties as Validator),
    "items" => Some(validate_items as Validator),
    "additionalItems" => Some(validate_additionalItems as Validator),
    "const" => Some(validate_const as Validator),
    "contains" => Some(validate_contains as Validator),
    "exclusiveMinimum" => Some(validate_exclusiveMinimum as Validator),
    "exclusiveMaximum" => Some(validate_exclusiveMaximum as Validator),
    "minimum" => Some(validate_minimum as Validator),
    "maximum" => Some(validate_maximum as Validator),
    "multipleOf" => Some(validate_multipleOf as Validator),
    "minItems" => Some(validate_minItems as Validator),
    "maxItems" => Some(validate_maxItems as Validator),
    "uniqueItems" => Some(validate_uniqueItems as Validator),
    "minLength" => Some(validate_minLength as Validator),
    "maxLength" => Some(validate_maxLength as Validator),
    "dependencies" => Some(validate_dependencies as Validator),
    "enum" => Some(validate_enum as Validator),
    "type" => Some(validate_type as Validator),
    "properties" => Some(validate_properties as Validator),
    "required" => Some(validate_required as Validator),
    "minProperties" => Some(validate_minProperties as Validator),
    "maxProperties" => Some(validate_maxProperties as Validator),
    "allOf" => Some(validate_allOf as Validator),
    "anyOf" => Some(validate_anyOf as Validator),
    "oneOf" => Some(validate_oneOf as Validator),
    "not" => Some(validate_not as Validator),
    _ => None
  }
}

pub fn run_validators(instance: &Value, schema: &Value) -> ValidatorResult {
  match schema {
    Value::Bool(b) => {
      if *b {
        Ok(())
      } else {
        Err(ValidationError::new("False schema always fails"))
      }
    },
    Value::Object(schema_object) => {
      if let Some(_sref) = schema_object.get("$ref") {
        Ok(()) // validate_ref(instance, sref, schema);
      } else {
        for (k, v) in schema_object.iter() {
          if let Some(validator) = get_validator(k.as_ref()) {
            if let Err(mut err) = validator(instance, v, schema_object) {
              err.schema_path.push(k.clone());
              return Err(err)
            }
          }
        }
        Ok(())
      }
    },
    _ => Err(ValidationError::new("Invalid schema"))
  }
}

pub fn is_valid(instance: &Value, schema: &Value) -> bool {
  run_validators(instance, schema).is_ok()
}

fn descend(instance: &Value, schema: &Value, instance_key: Option<&String>, schema_key: Option<&String>) -> ValidatorResult {
  if let Err(mut err) = run_validators(instance, schema) {
    if let Some(instance_key) = instance_key {
      err.instance_path.push(instance_key.clone());
    }
    if let Some(schema_key) = schema_key {
      err.schema_path.push(schema_key.clone());
    }
    Err(err)
  } else {
    Ok(())
  }
}

fn get_regex(pattern: &String) -> Result<regex::Regex, ValidationError> {
  match regex::Regex::new(pattern) {
    Ok(re) => Ok(re),
    Err(err) => match err {
      regex::Error::Syntax(msg) => Err(ValidationError::new(&msg)),
      regex::Error::CompiledTooBig(_) => Err(ValidationError::new("regex too big")),
      _ => Err(ValidationError::new("Unknown regular expression error"))
    }
  }
}

fn validate_patternProperties(instance: &Value, schema: &Value, _parent_schema: &Map<String, Value>) -> ValidatorResult {
  if let Value::Object(instance) = instance {
    if let Value::Object(schema) = schema {
      for (pattern, subschema) in schema.iter() {
        let re = get_regex(pattern)?;
        for (k, v) in instance.iter() {
          // TODO: Verify that regex syntax is the same
          if re.is_match(k) {
            descend(v, subschema, Some(k), Some(pattern))?;
          }
        }
      }
    }
  }
  Ok(())
}

fn validate_propertyNames(instance: &Value, schema: &Value, _parent_schema: &Map<String, Value>) -> ValidatorResult {
  if let Value::Object(instance) = instance {
    for (property, _) in instance.iter() {
      descend(&Value::String(property.to_string()), schema, Some(property), None)?;
    }
  }
  Ok(())
}

fn find_additional_properties<'a>(instance: &'a Map<String, Value>, schema: &'a Map<String, Value>) -> Box<Iterator<Item=&'a String> + 'a> {
  lazy_static! {
    static ref empty_obj: Value = Value::Object(Map::new());
  }
  let properties = schema.get("properties").unwrap_or_else(move || &empty_obj);
  let pattern_properties = schema.get("patternProperties").unwrap_or_else(move || &empty_obj);
  if let Value::Object(properties) = properties {
    if let Value::Object(pattern_properties) = pattern_properties {
      let pattern_regexes: Vec<regex::Regex> = pattern_properties
        .keys()
        .map(|k| get_regex(k).unwrap())
        .collect();
      return Box::new(
        instance
          .keys()
          .filter(
            move |&property| !properties.contains_key(property) &&
              !pattern_regexes.iter().any(|x| x.is_match(property))))
    }
  }
  Box::new(instance.keys())
}

fn validate_additionalProperties(instance: &Value, schema: &Value, parent_schema: &Map<String, Value>) -> ValidatorResult {
  if let Value::Object(instance) = instance {
    let mut extras = find_additional_properties(instance, parent_schema);
    match schema {
      Value::Object(_) => {
        for extra in extras {
          println!("extra {} schema {:?}", extra, schema);
          descend(instance.get(extra).expect("Property gone missing."), schema, Some(extra), None)?;
        }
      },
      Value::Bool(bool) => {
        if !bool {
          if let Some(_) = extras.next() {
            return Err(ValidationError::new("Additional properties are not allowed"))
          }
        }
      }
      _ => {}
    }
  }
  Ok(())
}

// TODO: items_draft3/4

fn validate_items(instance: &Value, schema: &Value, _parent_schema: &Map<String, Value>) -> ValidatorResult {
  if let Value::Array(instance) = instance {
    let items = bool_to_object_schema(schema);

    match items {
      Value::Object(_) =>
        for (index, item) in instance.iter().enumerate() {
          descend(item, items, Some(&index.to_string()), None)?;
        },
      Value::Array(items) =>
        for ((index, item), subschema) in instance.iter().enumerate().zip(items.iter()) {
          descend(item, subschema, Some(&index.to_string()), Some(&index.to_string()))?;
        },
      _ => {}
    }
  }
  Ok(())
}

fn validate_additionalItems(instance: &Value, schema: &Value, parent_schema: &Map<String, Value>) -> ValidatorResult {
  if !parent_schema.contains_key("items") {
    return Ok(())
  } else if let Value::Object(_) = parent_schema["items"] {
    return Ok(())
  }

  if let Value::Array(instance) = instance {
    let len_items = parent_schema.get("items").map_or(
      0,
      |x| match x { Value::Array(array) => array.len(), _ => 0 });
    match schema {
      Value::Object(_) =>
        for i in len_items..instance.len() {
          descend(&instance[i], schema, Some(&i.to_string()), None)?;
        },
      Value::Bool(b) =>
        if !b && instance.len() > len_items {
            return Err(ValidationError::new("Additional items are not allowed"))
        },
      _ => {}
    }
  }
  Ok(())
}

fn validate_const(instance: &Value, schema: &Value, _parent_schema: &Map<String, Value>) -> ValidatorResult {
  if instance != schema {
    return Err(ValidationError::new("Invalid const"))
  }
  Ok(())
}

fn validate_contains(instance: &Value, schema: &Value, _parent_schema: &Map<String, Value>) -> ValidatorResult {
  if let Value::Array(instance) = instance {
    if !instance.iter().any(|element| is_valid(element, schema)) {
      return Err(ValidationError::new("Nothing is valid under the given schema"))
    }
  }
  Ok(())
}

// TODO: minimum draft 3/4
// TODO: maximum draft 3/4

fn validate_exclusiveMinimum(instance: &Value, schema: &Value, _parent_schema: &Map<String, Value>) -> ValidatorResult {
  if let Value::Number(instance) = instance {
    if let Value::Number(schema) = schema {
      if instance.as_f64() <= schema.as_f64() {
        return Err(ValidationError::new("exclusiveMinimum"))
      }
    }
  }
  Ok(())
}

fn validate_exclusiveMaximum(instance: &Value, schema: &Value, _parent_schema: &Map<String, Value>) -> ValidatorResult {
  if let Value::Number(instance) = instance {
    if let Value::Number(schema) = schema {
      if instance.as_f64() >= schema.as_f64() {
        return Err(ValidationError::new("exclusiveMaximum"))
      }
    }
  }
  Ok(())
}

fn validate_minimum(instance: &Value, schema: &Value, _parent_schema: &Map<String, Value>) -> ValidatorResult {
  if let Value::Number(instance) = instance {
    if let Value::Number(schema) = schema {
      if instance.as_f64() < schema.as_f64() {
        return Err(ValidationError::new("minimum"))
      }
    }
  }
  Ok(())
}

fn validate_maximum(instance: &Value, schema: &Value, _parent_schema: &Map<String, Value>) -> ValidatorResult {
  if let Value::Number(instance) = instance {
    if let Value::Number(schema) = schema {
      if instance.as_f64() > schema.as_f64() {
        return Err(ValidationError::new("maximum"))
      }
    }
  }
  Ok(())
}

fn validate_multipleOf(instance: &Value, schema: &Value, _parent_schema: &Map<String, Value>) -> ValidatorResult {
  if let Value::Number(instance) = instance {
    if let Value::Number(schema) = schema {
      let failed = if schema.is_f64() {
        let quotient = instance.as_f64().unwrap() / schema.as_f64().unwrap();
        quotient.trunc() != quotient
      } else if schema.is_u64() {
        (instance.as_u64().unwrap() % schema.as_u64().unwrap()) != 0
      } else {
        (instance.as_i64().unwrap() % schema.as_i64().unwrap()) != 0
      };
      if failed {
        return Err(ValidationError::new("not multipleOf"))
      }
    }
  }
  Ok(())
}

fn validate_minItems(instance: &Value, schema: &Value, _parent_schema: &Map<String, Value>) -> ValidatorResult {
  if let Value::Array(instance) = instance {
    if let Value::Number(schema) = schema {
      if instance.len() < schema.as_u64().unwrap() as usize {
        return Err(ValidationError::new("minItems"))
      }
    }
  }
  Ok(())
}

fn validate_maxItems(instance: &Value, schema: &Value, _parent_schema: &Map<String, Value>) -> ValidatorResult {
  if let Value::Array(instance) = instance {
    if let Value::Number(schema) = schema {
      if instance.len() > schema.as_u64().unwrap() as usize {
        return Err(ValidationError::new("minItems"))
      }
    }
  }
  Ok(())
}

struct ValueWrapper<'a> {
  x: &'a Value
}

impl<'a> Hash for ValueWrapper<'a> {
  fn hash<H: Hasher>(&self, state: &mut H) {
    match self.x {
      Value::Array(array) =>
        for element in array {
          ValueWrapper { x: element }.hash(state);
        },
      Value::Object(object) =>
        for (key, val) in object {
          key.hash(state);
          ValueWrapper { x: val }.hash(state);
        },
      Value::String(string) => string.hash(state),
      Value::Number(number) => {
        if number.is_f64() {
          number.as_f64().unwrap().to_bits().hash(state);
        } else if number.is_u64() {
          number.as_u64().unwrap().hash(state);
        } else {
          number.as_i64().unwrap().hash(state);
        }
      },
      Value::Bool(bool) => bool.hash(state),
      Value::Null => 0.hash(state)
    }
  }
}

impl<'a> PartialEq for ValueWrapper<'a> {
  fn eq(&self, other: &ValueWrapper<'a>) -> bool {
    self.x == other.x
  }
}

impl<'a> Eq for ValueWrapper<'a> {}

fn has_unique_elements<T>(iter: T) -> bool
where
  T: IntoIterator,
  T::Item: Eq + Hash,
{
  let mut uniq = HashSet::new();
  iter.into_iter().all(move |x| uniq.insert(x))
}

fn validate_uniqueItems(instance: &Value, schema: &Value, _parent_schema: &Map<String, Value>) -> ValidatorResult {
  if let Value::Array(instance) = instance {
    if let Value::Bool(b) = schema {
      if *b && !has_unique_elements(instance.iter().map(|x| ValueWrapper {x: x})) {
        return Err(ValidationError::new("uniqueItems"))
      }
    }
  }
  Ok(())
}

// TODO pattern

// TODO format

fn validate_minLength(instance: &Value, schema: &Value, _parent_schema: &Map<String, Value>) -> ValidatorResult {
  if let Value::String(instance) = instance {
    if let Value::Number(schema) = schema {
      if instance.chars().count() < schema.as_u64().unwrap() as usize {
        return Err(ValidationError::new("minLength"))
      }
    }
  }
  Ok(())
}

fn validate_maxLength(instance: &Value, schema: &Value, _parent_schema: &Map<String, Value>) -> ValidatorResult {
  if let Value::String(instance) = instance {
    if let Value::Number(schema) = schema {
      if instance.chars().count() > schema.as_u64().unwrap() as usize {
        return Err(ValidationError::new("maxLength"))
      }
    }
  }
  Ok(())
}

fn bool_to_object_schema<'a>(schema: &'a Value) -> &'a Value {
  lazy_static! {
    static ref empty_schema: Value = Value::Object(Map::new());
    static ref inverse_schema: Value = json!({"not": {}});
  }

  match schema {
    Value::Bool(bool) => {
      if *bool {
        &empty_schema
      } else {
        &inverse_schema
      }
    },
    _ => schema
  }
}

fn iter_or_once<'a>(instance: &'a Value) -> Box<Iterator<Item=&'a Value> + 'a> {
  match instance {
    Value::Array(array) => Box::new(array.iter()),
    _ => Box::new(iter::once(instance))
  }
}

fn validate_dependencies(instance: &Value, schema: &Value, _parent_schema: &Map<String, Value>) -> ValidatorResult {
  if let Value::Object(object) = instance {
    if let Value::Object(schema) = schema {
      for (property, dependency) in schema.iter() {
        let dep = bool_to_object_schema(dependency);
        match dep {
          Value::Object(_) =>
            descend(instance, dep, None, Some(property))?,
          _ => {
            for dep0 in iter_or_once(dep) {
              if let Value::String(key) = dep0 {
                if !object.contains_key(key) {
                  return Err(ValidationError::new("dependency"))
                }
              }
            }
          }
        }
      }
    }
  }
  Ok(())
}


fn validate_enum(instance: &Value, schema: &Value, _parent_schema: &Map<String, Value>) -> ValidatorResult {
  if let Value::Array(enums) = schema {
    if !enums.iter().any(|val| val == instance) {
      return Err(ValidationError::new("enum"))
    }
  }
  Ok(())
}

// TODO: ref

// TODO: type draft3
// TODO: properties draft3
// TODO: disallow draft3
// TODO: extends draft3

fn validate_single_type(instance: &Value, schema: &Value) -> ValidatorResult {
  if let Value::String(typename) = schema {
    match typename.as_ref() {
      "array" => {
        if let Value::Array(_) = instance {
          return Ok(())
        } else {
          return Err(ValidationError::new("array"))
        }
      },
      "object" => {
        if let Value::Object(_) = instance {
          return Ok(())
        } else {
          return Err(ValidationError::new("object"))
        }
      },
      "null" => {
        if let Value::Null = instance {
          return Ok(())
        } else {
          return Err(ValidationError::new("null"))
        }
      },
      "number" => {
        if let Value::Number(_) = instance {
          return Ok(())
        } else {
          return Err(ValidationError::new("number"))
        }
      },
      "string" => {
        if let Value::String(_) = instance {
          return Ok(())
        } else {
          return Err(ValidationError::new("string"))
        }
      },
      "integer" => {
        if let Value::Number(number) = instance {
          if number.is_i64() || number.is_u64() ||
            (number.is_f64() && number.as_f64().unwrap().trunc() == number.as_f64().unwrap()) {
            return Ok(())
          }
        }
        return Err(ValidationError::new("integer"))
      },
      "boolean" => {
        if let Value::Bool(_) = instance {
          return Ok(())
        } else {
          return Err(ValidationError::new("boolean"))
        }
      }
      _ => return Ok(())
    }
  }
  Ok(())
}

fn validate_type(instance: &Value, schema: &Value, _parent_schema: &Map<String, Value>) -> ValidatorResult {
  if !iter_or_once(schema).any(|x| validate_single_type(instance, x).is_ok()) {
    return Err(ValidationError::new("type"))
  }
  Ok(())
}

fn validate_properties(instance: &Value, schema: &Value, _parent_schema: &Map<String, Value>) -> ValidatorResult {
  if let Value::Object(instance) = instance {
    if let Value::Object(schema) = schema {
      for (property, subschema) in schema.iter() {
        if instance.contains_key(property) {
          descend(instance.get(property).unwrap(), subschema, Some(property), Some(property))?;
        }
      }
    }
  }
  Ok(())
}

fn validate_required(instance: &Value, schema: &Value, _parent_schema: &Map<String, Value>) -> ValidatorResult {
  if let Value::Object(instance) = instance {
    if let Value::Array(schema) = schema {
      for property in schema.iter() {
        if let Value::String(key) = property {
          if !instance.contains_key(key) {
            return Err(ValidationError::new(
              &format!("required property '{}' missing", key)))
          }
        }
      }
    }
  }
  Ok(())
}

fn validate_minProperties(instance: &Value, schema: &Value, _parent_schema: &Map<String, Value>) -> ValidatorResult {
  if let Value::Object(instance) = instance {
    if let Value::Number(schema) = schema {
      if instance.len() < schema.as_u64().unwrap() as usize {
        return Err(ValidationError::new("minProperties"))
      }
    }
  }
  Ok(())
}

fn validate_maxProperties(instance: &Value, schema: &Value, _parent_schema: &Map<String, Value>) -> ValidatorResult {
  if let Value::Object(instance) = instance {
    if let Value::Number(schema) = schema {
      if instance.len() > schema.as_u64().unwrap() as usize {
        return Err(ValidationError::new("maxProperties"))
      }
    }
  }
  Ok(())
}

fn validate_allOf(instance: &Value, schema: &Value, _parent_schema: &Map<String, Value>) -> ValidatorResult {
  if let Value::Array(schema) = schema {
    for (index, subschema) in schema.iter().enumerate() {
      let subschema0 = bool_to_object_schema(subschema);
      descend(instance, subschema0, None, Some(&index.to_string()))?;
    }
  }
  Ok(())
}

fn validate_anyOf(instance: &Value, schema: &Value, _parent_schema: &Map<String, Value>) -> ValidatorResult {
  if let Value::Array(schema) = schema {
    for (index, subschema) in schema.iter().enumerate() {
      let subschema0 = bool_to_object_schema(subschema);
      // TODO Wrap up all errors into a list
      if descend(instance, subschema0, None, Some(&index.to_string())).is_ok() {
        return Ok(())
      }
      return Err(ValidationError::new("anyOf"))
    }
  }
  Ok(())
}

fn validate_oneOf(_instance: &Value, _schema: &Value, _parent_schema: &Map<String, Value>) -> ValidatorResult {
  // TODO
  Ok(())
}

fn validate_not(instance: &Value, schema: &Value, _parent_schema: &Map<String, Value>) -> ValidatorResult {
  if run_validators(instance, schema).is_ok() {
    return Err(ValidationError::new("not"))
  }
  Ok(())
}