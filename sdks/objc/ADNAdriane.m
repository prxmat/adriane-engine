#import "ADNAdriane.h"
#import "../../crates/c-api/include/adriane.h"

static NSString * const ADNAdrianeErrorDomain = @"ai.adriane";

@implementation ADNAdriane

+ (NSString *)engineVersion {
  char *ptr = adriane_engine_version();
  if (ptr == NULL) {
    return @"";
  }
  NSString *value = [NSString stringWithUTF8String:ptr] ?: @"";
  adriane_string_free(ptr);
  return value;
}

+ (nullable NSString *)validateGraphJSON:(NSString *)definitionJSON error:(NSError **)error {
  return [self unwrap:adriane_validate_graph_json(definitionJSON.UTF8String) error:error];
}

+ (nullable NSString *)compileGraphYAMLJSON:(NSString *)yaml error:(NSError **)error {
  return [self unwrap:adriane_compile_graph_yaml_json(yaml.UTF8String) error:error];
}

+ (nullable NSString *)availableProvidersJSON:(NSError **)error {
  return [self unwrap:adriane_available_providers_json() error:error];
}

+ (nullable NSString *)resolveModelJSONWithTier:(NSString *)tier
                                  availableJSON:(nullable NSString *)availableJSON
                                   overrideJSON:(nullable NSString *)overrideJSON
                                          error:(NSError **)error {
  return [self unwrap:adriane_resolve_model_json(tier.UTF8String, availableJSON.UTF8String, overrideJSON.UTF8String) error:error];
}

+ (nullable NSString *)listComponentsJSON:(NSError **)error {
  return [self unwrap:adriane_list_components_json() error:error];
}

+ (nullable NSString *)listPrebuiltJSON:(NSError **)error {
  return [self unwrap:adriane_list_prebuilt_json() error:error];
}

+ (nullable NSString *)runComponentJSONWithKind:(NSString *)kind
                                     paramsJSON:(NSString *)paramsJSON
                                   channelsJSON:(NSString *)channelsJSON
                                          error:(NSError **)error {
  return [self unwrap:adriane_run_component_json(kind.UTF8String, paramsJSON.UTF8String, channelsJSON.UTF8String) error:error];
}

+ (nullable NSString *)runPrebuiltJSONWithName:(NSString *)name
                                     inputJSON:(NSString *)inputJSON
                                   optionsJSON:(nullable NSString *)optionsJSON
                                         error:(NSError **)error {
  return [self unwrap:adriane_run_prebuilt_json(name.UTF8String, inputJSON.UTF8String, optionsJSON.UTF8String) error:error];
}

+ (nullable NSString *)unwrap:(AdrianeResult)result error:(NSError **)error {
  if (result.code == ADRIANE_OK) {
    NSString *value = result.value == NULL ? @"" : [NSString stringWithUTF8String:result.value];
    adriane_result_free(result);
    return value ?: @"";
  }

  NSString *message = result.error == NULL
      ? [NSString stringWithFormat:@"Adriane C API error %d", result.code]
      : [NSString stringWithUTF8String:result.error];
  if (error != nil) {
    *error = [NSError errorWithDomain:ADNAdrianeErrorDomain
                                 code:result.code
                             userInfo:@{NSLocalizedDescriptionKey: message ?: @"Adriane C API error"}];
  }
  adriane_result_free(result);
  return nil;
}

@end
