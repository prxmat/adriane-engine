<?php

final class Adriane
{
    private \FFI $ffi;

    private function __construct(\FFI $ffi)
    {
        $this->ffi = $ffi;
    }

    public static function load(?string $library = null): self
    {
        $library = $library ?: self::defaultLibraryName();
        $cdef = <<<'CDEF'
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
CDEF;
        return new self(\FFI::cdef($cdef, $library));
    }

    public function engineVersion(): string
    {
        $ptr = $this->ffi->adriane_engine_version();
        if ($ptr === null) {
            throw new \RuntimeException("adriane_engine_version returned null");
        }

        try {
            return \FFI::string($ptr);
        } finally {
            $this->ffi->adriane_string_free($ptr);
        }
    }

    public function validateGraphJson(string $definitionJson): string
    {
        return $this->unwrap($this->ffi->adriane_validate_graph_json($definitionJson));
    }

    public function compileGraphYamlJson(string $yaml): string
    {
        return $this->unwrap($this->ffi->adriane_compile_graph_yaml_json($yaml));
    }

    public function availableProvidersJson(): string
    {
        return $this->unwrap($this->ffi->adriane_available_providers_json());
    }

    public function resolveModelJson(string $tier, ?string $availableJson = null, ?string $overrideJson = null): string
    {
        return $this->unwrap($this->ffi->adriane_resolve_model_json($tier, $availableJson, $overrideJson));
    }

    public function listComponentsJson(): string
    {
        return $this->unwrap($this->ffi->adriane_list_components_json());
    }

    public function listPrebuiltJson(): string
    {
        return $this->unwrap($this->ffi->adriane_list_prebuilt_json());
    }

    public function runComponentJson(string $kind, string $paramsJson, string $channelsJson): string
    {
        return $this->unwrap($this->ffi->adriane_run_component_json($kind, $paramsJson, $channelsJson));
    }

    public function runPrebuiltJson(string $name, string $inputJson, ?string $optionsJson = null): string
    {
        return $this->unwrap($this->ffi->adriane_run_prebuilt_json($name, $inputJson, $optionsJson));
    }

    private function unwrap(\FFI\CData $result): string
    {
        try {
            if ($result->code === 0) {
                return \FFI::string($result->value);
            }

            $message = $result->error === null
                ? "Adriane C API error {$result->code}"
                : \FFI::string($result->error);
            throw new \RuntimeException($message);
        } finally {
            $this->ffi->adriane_result_free($result);
        }
    }

    private static function defaultLibraryName(): string
    {
        return match (PHP_OS_FAMILY) {
            "Darwin" => "libadriane_c_api.dylib",
            "Windows" => "adriane_c_api.dll",
            default => "libadriane_c_api.so",
        };
    }
}
