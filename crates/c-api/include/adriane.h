#ifndef ADRIANE_H
#define ADRIANE_H

#ifdef __cplusplus
extern "C" {
#endif

#define ADRIANE_OK 0
#define ADRIANE_ERR_NULL 1
#define ADRIANE_ERR_UTF8 2
#define ADRIANE_ERR_INPUT 3
#define ADRIANE_ERR_INTERNAL 4

typedef struct AdrianeResult {
  int code;
  char *value;
  char *error;
} AdrianeResult;

char *adriane_engine_version(void);
AdrianeResult adriane_validate_graph_json(const char *definition_json);
AdrianeResult adriane_compile_graph_yaml_json(const char *yaml);
AdrianeResult adriane_available_providers_json(void);
AdrianeResult adriane_resolve_model_json(const char *tier, const char *available_json, const char *override_json);
AdrianeResult adriane_list_components_json(void);
AdrianeResult adriane_list_prebuilt_json(void);
AdrianeResult adriane_run_component_json(const char *kind, const char *params_json, const char *channels_json);
AdrianeResult adriane_run_prebuilt_json(const char *name, const char *input_json, const char *options_json);
void adriane_string_free(char *ptr);
void adriane_result_free(AdrianeResult result);

#ifdef __cplusplus
}
#endif

#endif
