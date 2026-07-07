package ai.adriane;

import com.sun.jna.Library;
import com.sun.jna.Native;
import com.sun.jna.Pointer;
import com.sun.jna.Structure;

import java.util.Arrays;
import java.util.List;

public final class Adriane {
  private Adriane() {}

  public static final class AdrianeException extends RuntimeException {
    public final int code;

    public AdrianeException(int code, String message) {
      super(message);
      this.code = code;
    }
  }

  public static final class AdrianeResult extends Structure implements Structure.ByValue {
    public int code;
    public Pointer value;
    public Pointer error;

    @Override
    protected List<String> getFieldOrder() {
      return Arrays.asList("code", "value", "error");
    }
  }

  private interface NativeAdriane extends Library {
    NativeAdriane INSTANCE = Native.load(
        System.getenv().getOrDefault("ADRIANE_C_API_LIB", "adriane_c_api"),
        NativeAdriane.class
    );

    Pointer adriane_engine_version();
    AdrianeResult adriane_validate_graph_json(String definitionJson);
    AdrianeResult adriane_compile_graph_yaml_json(String yaml);
    AdrianeResult adriane_available_providers_json();
    AdrianeResult adriane_resolve_model_json(String tier, String availableJson, String overrideJson);
    AdrianeResult adriane_list_components_json();
    AdrianeResult adriane_list_prebuilt_json();
    AdrianeResult adriane_run_component_json(String kind, String paramsJson, String channelsJson);
    AdrianeResult adriane_run_prebuilt_json(String name, String inputJson, String optionsJson);
    void adriane_string_free(Pointer ptr);
    void adriane_result_free(AdrianeResult result);
  }

  public static String engineVersion() {
    Pointer ptr = NativeAdriane.INSTANCE.adriane_engine_version();
    if (ptr == null) {
      return "";
    }
    try {
      return ptr.getString(0, "UTF-8");
    } finally {
      NativeAdriane.INSTANCE.adriane_string_free(ptr);
    }
  }

  public static String validateGraphJson(String definitionJson) {
    return unwrap(NativeAdriane.INSTANCE.adriane_validate_graph_json(definitionJson));
  }

  public static String compileGraphYamlJson(String yaml) {
    return unwrap(NativeAdriane.INSTANCE.adriane_compile_graph_yaml_json(yaml));
  }

  public static String availableProvidersJson() {
    return unwrap(NativeAdriane.INSTANCE.adriane_available_providers_json());
  }

  public static String resolveModelJson(String tier, String availableJson, String overrideJson) {
    return unwrap(NativeAdriane.INSTANCE.adriane_resolve_model_json(tier, availableJson, overrideJson));
  }

  public static String listComponentsJson() {
    return unwrap(NativeAdriane.INSTANCE.adriane_list_components_json());
  }

  public static String listPrebuiltJson() {
    return unwrap(NativeAdriane.INSTANCE.adriane_list_prebuilt_json());
  }

  public static String runComponentJson(String kind, String paramsJson, String channelsJson) {
    return unwrap(NativeAdriane.INSTANCE.adriane_run_component_json(kind, paramsJson, channelsJson));
  }

  public static String runPrebuiltJson(String name, String inputJson, String optionsJson) {
    return unwrap(NativeAdriane.INSTANCE.adriane_run_prebuilt_json(name, inputJson, optionsJson));
  }

  private static String unwrap(AdrianeResult result) {
    try {
      if (result.code == 0) {
        return result.value == null ? "" : result.value.getString(0, "UTF-8");
      }

      String message = result.error == null
          ? "Adriane C API error " + result.code
          : result.error.getString(0, "UTF-8");
      throw new AdrianeException(result.code, message);
    } finally {
      NativeAdriane.INSTANCE.adriane_result_free(result);
    }
  }
}
