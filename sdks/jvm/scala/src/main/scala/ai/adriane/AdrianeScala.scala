package ai.adriane

object AdrianeScala {
  def engineVersion(): String = Adriane.engineVersion()
  def validateGraphJson(definitionJson: String): String = Adriane.validateGraphJson(definitionJson)
  def compileGraphYamlJson(yaml: String): String = Adriane.compileGraphYamlJson(yaml)
  def availableProvidersJson(): String = Adriane.availableProvidersJson()
  def resolveModelJson(tier: String, availableJson: String = null, overrideJson: String = null): String =
    Adriane.resolveModelJson(tier, availableJson, overrideJson)
  def listComponentsJson(): String = Adriane.listComponentsJson()
  def listPrebuiltJson(): String = Adriane.listPrebuiltJson()
  def runComponentJson(kind: String, paramsJson: String, channelsJson: String): String =
    Adriane.runComponentJson(kind, paramsJson, channelsJson)
  def runPrebuiltJson(name: String, inputJson: String, optionsJson: String = null): String =
    Adriane.runPrebuiltJson(name, inputJson, optionsJson)
}
