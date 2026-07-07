package ai.adriane;

import com.sun.jna.Callback;
import com.sun.jna.Library;
import com.sun.jna.Memory;
import com.sun.jna.Native;
import com.sun.jna.Pointer;
import com.sun.jna.Structure;
import com.sun.jna.ptr.PointerByReference;

import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.Collections;
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

  public static final class AdrianeCallbackResult {
    private static final List<Memory> CALLBACK_STRINGS = Collections.synchronizedList(new ArrayList<>());

    public final int code;
    public final Pointer value;
    public final Pointer error;

    private AdrianeCallbackResult(int code, Pointer value, Pointer error) {
      this.code = code;
      this.value = value;
      this.error = error;
    }

    public static AdrianeCallbackResult ok(String value) {
      return new AdrianeCallbackResult(0, callbackString(value), null);
    }

    public static AdrianeCallbackResult error(String message) {
      return new AdrianeCallbackResult(3, null, callbackString(message));
    }

    int writeTo(PointerByReference valueOut, PointerByReference errorOut) {
      valueOut.setValue(value);
      errorOut.setValue(error);
      return code;
    }

    private static Pointer callbackString(String value) {
      if (value == null) {
        value = "";
      }
      byte[] bytes = value.getBytes(StandardCharsets.UTF_8);
      Memory memory = new Memory(bytes.length + 1L);
      memory.write(0, bytes, 0, bytes.length);
      memory.setByte(bytes.length, (byte) 0);
      CALLBACK_STRINGS.add(memory);
      return memory;
    }
  }

  @FunctionalInterface
  public interface HostStringCallback {
    AdrianeCallbackResult invoke(String payloadJson, Pointer userData);
  }

  public interface StringCallback extends Callback {
    int invoke(String payloadJson, Pointer userData, PointerByReference value, PointerByReference error);
  }

  public interface EventCallback extends Callback {
    void invoke(String payloadJson, Pointer userData);
  }

  public static final class AdrianeCallbacks extends Structure implements Structure.ByValue {
    public Pointer userData;
    public StringCallback onNode;
    public StringCallback onCondition;
    public EventCallback onEvent;

    public AdrianeCallbacks() {
      this.userData = null;
    }

    public AdrianeCallbacks(HostStringCallback onNode, HostStringCallback onCondition, EventCallback onEvent) {
      this.userData = null;
      this.onNode = stringCallback(onNode);
      this.onCondition = stringCallback(onCondition);
      this.onEvent = onEvent;
    }

    @Override
    protected List<String> getFieldOrder() {
      return Arrays.asList("userData", "onNode", "onCondition", "onEvent");
    }
  }

  private static StringCallback stringCallback(HostStringCallback callback) {
    return (payloadJson, userData, value, error) -> {
      try {
        AdrianeCallbackResult result = callback.invoke(payloadJson, userData);
        if (result == null) {
          result = AdrianeCallbackResult.error("callback returned null");
        }
        return result.writeTo(value, error);
      } catch (RuntimeException ex) {
        String message = ex.getMessage() == null ? ex.getClass().getSimpleName() : ex.getMessage();
        return AdrianeCallbackResult.error(message).writeTo(value, error);
      }
    };
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
    AdrianeResult adriane_engine_run_json(String specJson, AdrianeCallbacks callbacks);
    AdrianeResult adriane_engine_resume_json(String specJson, AdrianeCallbacks callbacks);
    AdrianeResult adriane_engine_approve_and_resume_json(String specJson, AdrianeCallbacks callbacks);
    AdrianeResult adriane_engine_signal_json(String specJson, String signalName, String payloadJson, AdrianeCallbacks callbacks);
    AdrianeResult adriane_engine_replay_json(String specJson, String checkpointId, AdrianeCallbacks callbacks);
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

  public static String engineRunJson(String specJson, AdrianeCallbacks callbacks) {
    return unwrap(NativeAdriane.INSTANCE.adriane_engine_run_json(specJson, callbacks));
  }

  public static String engineResumeJson(String specJson, AdrianeCallbacks callbacks) {
    return unwrap(NativeAdriane.INSTANCE.adriane_engine_resume_json(specJson, callbacks));
  }

  public static String engineApproveAndResumeJson(String specJson, AdrianeCallbacks callbacks) {
    return unwrap(NativeAdriane.INSTANCE.adriane_engine_approve_and_resume_json(specJson, callbacks));
  }

  public static String engineSignalJson(String specJson, String signalName, String payloadJson, AdrianeCallbacks callbacks) {
    return unwrap(NativeAdriane.INSTANCE.adriane_engine_signal_json(specJson, signalName, payloadJson, callbacks));
  }

  public static String engineReplayJson(String specJson, String checkpointId, AdrianeCallbacks callbacks) {
    return unwrap(NativeAdriane.INSTANCE.adriane_engine_replay_json(specJson, checkpointId, callbacks));
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
