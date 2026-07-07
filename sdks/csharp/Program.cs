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

Console.WriteLine("csharp ok");
