using Adriane;

if (string.IsNullOrWhiteSpace(Adriane.Adriane.EngineVersion()))
{
    throw new Exception("empty engine version");
}

if (!Adriane.Adriane.ListComponentsJson().Contains("promptBuilder", StringComparison.Ordinal))
{
    throw new Exception("component catalog missing promptBuilder");
}

var output = Adriane.Adriane.RunComponentJson(
    "promptBuilder",
    """{"template":"Hello {{name}}!","into":"prompt"}""",
    """{"name":"Ada"}"""
);

if (output != """{"prompt":"Hello Ada!"}""")
{
    throw new Exception($"unexpected component output: {output}");
}

using var callbacks = new Adriane.Adriane.EngineCallbacks(
    onNode: _ => """{"greeting":"hello from csharp"}""",
    onCondition: _ => "true"
);

var run = Adriane.Adriane.EngineRunJson(
    """
    {
      "graph": {
        "id": "csharp-callback",
        "version": "1.0.0",
        "name": "CSharp callback",
        "entryNodeId": "start",
        "channels": {
          "greeting": { "type": "string", "reducer": "replace" }
        },
        "nodes": [
          { "id": "start", "type": "action", "label": "Start" }
        ],
        "edges": []
      },
      "runId": "run-csharp",
      "jsNodeIds": ["start"]
    }
    """,
    callbacks
);

if (!run.Contains("hello from csharp", StringComparison.Ordinal))
{
    throw new Exception($"unexpected callback run output: {run}");
}

Console.WriteLine("csharp ok");
