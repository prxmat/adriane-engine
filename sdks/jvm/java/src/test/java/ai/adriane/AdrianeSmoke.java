package ai.adriane;

import java.util.ArrayList;
import java.util.List;

public final class AdrianeSmoke {
  private AdrianeSmoke() {}

  public static void main(String[] args) {
    if (Adriane.engineVersion().isBlank()) {
      throw new IllegalStateException("empty engine version");
    }

    String components = Adriane.listComponentsJson();
    if (!components.contains("promptBuilder")) {
      throw new IllegalStateException("component catalog missing promptBuilder: " + components);
    }

    String output = Adriane.runComponentJson(
        "promptBuilder",
        "{\"template\":\"Hello {{name}}!\",\"into\":\"prompt\"}",
        "{\"name\":\"Ada\"}"
    );
    if (!"{\"prompt\":\"Hello Ada!\"}".equals(output)) {
      throw new IllegalStateException("unexpected component output: " + output);
    }

    List<String> events = new ArrayList<>();
    Adriane.AdrianeCallbacks callbacks = new Adriane.AdrianeCallbacks(
        (payload, userData) -> Adriane.AdrianeCallbackResult.ok("{\"greeting\":\"hello from java\"}"),
        (payload, userData) -> Adriane.AdrianeCallbackResult.ok("true"),
        (payload, userData) -> events.add(payload)
    );
    String run = Adriane.engineRunJson("""
        {
          "graph": {
            "id": "java-callback",
            "version": "1.0.0",
            "name": "Java callback",
            "entryNodeId": "start",
            "channels": {
              "greeting": { "type": "string", "reducer": "replace" }
            },
            "nodes": [
              { "id": "start", "type": "action", "label": "Start" }
            ],
            "edges": []
          },
          "runId": "run-java",
          "jsNodeIds": ["start"]
        }
        """, callbacks);
    if (!run.contains("hello from java")) {
      throw new IllegalStateException("unexpected callback run output: " + run + " events=" + events);
    }

    System.out.println("java ok");
  }
}
