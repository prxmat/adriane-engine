package ai.adriane

object AdrianeKt {
    fun engineVersion(): String = Adriane.engineVersion()
    fun validateGraphJson(definitionJson: String): String = Adriane.validateGraphJson(definitionJson)
    fun compileGraphYamlJson(yaml: String): String = Adriane.compileGraphYamlJson(yaml)
    fun availableProvidersJson(): String = Adriane.availableProvidersJson()
    fun resolveModelJson(tier: String, availableJson: String? = null, overrideJson: String? = null): String =
        Adriane.resolveModelJson(tier, availableJson, overrideJson)
    fun listComponentsJson(): String = Adriane.listComponentsJson()
    fun listPrebuiltJson(): String = Adriane.listPrebuiltJson()
    fun runComponentJson(kind: String, paramsJson: String, channelsJson: String): String =
        Adriane.runComponentJson(kind, paramsJson, channelsJson)
    fun runPrebuiltJson(name: String, inputJson: String, optionsJson: String? = null): String =
        Adriane.runPrebuiltJson(name, inputJson, optionsJson)
    fun engineRunJson(specJson: String, callbacks: Adriane.AdrianeCallbacks): String =
        Adriane.engineRunJson(specJson, callbacks)
    fun engineResumeJson(specJson: String, callbacks: Adriane.AdrianeCallbacks): String =
        Adriane.engineResumeJson(specJson, callbacks)
    fun engineApproveAndResumeJson(specJson: String, callbacks: Adriane.AdrianeCallbacks): String =
        Adriane.engineApproveAndResumeJson(specJson, callbacks)
    fun engineSignalJson(specJson: String, signalName: String, payloadJson: String, callbacks: Adriane.AdrianeCallbacks): String =
        Adriane.engineSignalJson(specJson, signalName, payloadJson, callbacks)
    fun engineReplayJson(specJson: String, checkpointId: String, callbacks: Adriane.AdrianeCallbacks): String =
        Adriane.engineReplayJson(specJson, checkpointId, callbacks)
}
