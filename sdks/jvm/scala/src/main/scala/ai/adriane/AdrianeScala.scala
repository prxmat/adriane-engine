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
  def engineRunJson(specJson: String, callbacks: Adriane.AdrianeCallbacks): String =
    Adriane.engineRunJson(specJson, callbacks)
  def engineResumeJson(specJson: String, callbacks: Adriane.AdrianeCallbacks): String =
    Adriane.engineResumeJson(specJson, callbacks)
  def engineApproveAndResumeJson(specJson: String, callbacks: Adriane.AdrianeCallbacks): String =
    Adriane.engineApproveAndResumeJson(specJson, callbacks)
  def engineSignalJson(specJson: String, signalName: String, payloadJson: String, callbacks: Adriane.AdrianeCallbacks): String =
    Adriane.engineSignalJson(specJson, signalName, payloadJson, callbacks)
  def engineReplayJson(specJson: String, checkpointId: String, callbacks: Adriane.AdrianeCallbacks): String =
    Adriane.engineReplayJson(specJson, checkpointId, callbacks)
}
