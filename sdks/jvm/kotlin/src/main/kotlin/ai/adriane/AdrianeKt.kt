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
}
