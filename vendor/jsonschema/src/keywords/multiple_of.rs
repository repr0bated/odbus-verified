use crate::{
    compiler,
    error::ValidationError,
    keywords::CompilationResult,
    paths::{LazyLocation, Location},
    primitive_type::PrimitiveType,
    validator::Validate,
};
use fraction::{BigFraction, BigUint};
use serde_json::{Map, Number, Value};

struct MultipleOfValidator {
    multiple_of: f64,
    divisor: BigFraction,
    integer_divisor: Option<u64>,
    location: Location,
}

// Preserve the decimal value represented by serde_json, including exact i64/u64
// values. Converting integers to f64 or dividing binary floats first loses the
// information needed to decide divisibility.
fn decimal_fraction(number: &Number) -> BigFraction {
    if let Some(number) = number.as_u64() {
        return BigFraction::from(number);
    }
    if let Some(number) = number.as_i64() {
        return BigFraction::from(number);
    }
    let decimal = number.to_string();
    let (mantissa, exponent) = match decimal.split_once(['e', 'E']) {
        Some((mantissa, exponent)) => (
            mantissa,
            exponent.parse::<i32>().expect("Finite JSON number exponent"),
        ),
        None => (decimal.as_str(), 0),
    };
    let fraction = mantissa
        .parse::<BigFraction>()
        .expect("JSON number mantissa is a decimal");
    if exponent == 0 {
        return fraction;
    }
    let scale = BigFraction::from(BigUint::from(10_u8).pow(exponent.unsigned_abs()));
    if exponent > 0 {
        fraction * scale
    } else {
        fraction / scale
    }
}

impl Validate for MultipleOfValidator {
    fn is_valid(&self, instance: &Value) -> bool {
        if let Value::Number(item) = instance {
            if let Some(divisor) = self.integer_divisor {
                if let Some(item) = item.as_u64() {
                    return item % divisor == 0;
                }
                if let Some(item) = item.as_i64() {
                    return item.unsigned_abs() % divisor == 0;
                }
            }
            let quotient = &decimal_fraction(item) / &self.divisor;
            quotient.denom() == Some(&BigUint::from(1_u8))
        } else {
            true
        }
    }

    fn validate<'i>(
        &self,
        instance: &'i Value,
        location: &LazyLocation,
    ) -> Result<(), ValidationError<'i>> {
        if !self.is_valid(instance) {
            return Err(ValidationError::multiple_of(
                self.location.clone(),
                location.into(),
                instance,
                self.multiple_of,
            ));
        }
        Ok(())
    }
}

#[inline]
pub(crate) fn compile<'a>(
    ctx: &compiler::Context,
    _: &'a Map<String, Value>,
    schema: &'a Value,
) -> Option<CompilationResult<'a>> {
    if let Value::Number(multiple_of) = schema {
        let value = multiple_of.as_f64().expect("Always valid");
        let location = ctx.location().join("multipleOf");
        if value <= 0.0 {
            return Some(Err(ValidationError::exclusive_minimum(
                Location::new(),
                location,
                schema,
                Value::from(0),
            )));
        }
        Some(Ok(Box::new(MultipleOfValidator {
            multiple_of: value,
            divisor: decimal_fraction(multiple_of),
            integer_divisor: multiple_of.as_u64(),
            location,
        })))
    } else {
        Some(Err(ValidationError::single_type_error(
            Location::new(),
            ctx.location().clone(),
            schema,
            PrimitiveType::Number,
        )))
    }
}

#[cfg(test)]
mod tests {
    use crate::tests_util;
    use serde_json::{json, Value};
    use test_case::test_case;

    #[test_case(&json!({"multipleOf": 2}), &json!(4))]
    #[test_case(&json!({"multipleOf": 1.0}), &json!(4.0))]
    #[test_case(&json!({"multipleOf": 1.5}), &json!(3.0))]
    #[test_case(&json!({"multipleOf": 1.5}), &json!(4.5))]
    #[test_case(&json!({"multipleOf": 0.1}), &json!(0.3))]
    #[test_case(&json!({"multipleOf": 0.1}), &json!(-0.3))]
    #[test_case(&json!({"multipleOf": 1e-20}), &json!(3e-20))]
    #[test_case(&json!({"multipleOf": 1e20}), &json!(3e20))]
    #[test_case(&json!({"multipleOf": 2}), &json!(i64::MIN))]
    #[test_case(&json!({"multipleOf": 9007199254740993_u64}), &json!(9007199254740993_u64))]
    fn multiple_of_is_valid(schema: &Value, instance: &Value) {
        tests_util::is_valid(schema, instance)
    }

    #[test_case(&json!({"multipleOf": 1.0}), &json!(4.5))]
    #[test_case(&json!({"multipleOf": 1.5}), &json!(-4.0))]
    #[test_case(&json!({"multipleOf": 0.1}), &json!(0.30000000000000004))]
    #[test_case(&json!({"multipleOf": 2}), &json!(9007199254740993_u64))]
    #[test_case(&json!({"multipleOf": 2}), &json!(u64::MAX))]
    #[test_case(&json!({"multipleOf": 9007199254740993_u64}), &json!(9007199254740992_u64))]
    fn multiple_of_is_not_valid(schema: &Value, instance: &Value) {
        tests_util::is_not_valid(schema, instance)
    }

    #[test_case(&json!({"multipleOf": 2}), &json!(3), "/multipleOf")]
    #[test_case(&json!({"multipleOf": 1.5}), &json!(5), "/multipleOf")]
    fn location(schema: &Value, instance: &Value, expected: &str) {
        tests_util::assert_schema_location(schema, instance, expected)
    }
}
