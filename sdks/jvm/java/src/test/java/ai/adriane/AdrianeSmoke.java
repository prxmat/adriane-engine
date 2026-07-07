package ai.adriane;

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

    System.out.println("java ok");
  }
}
