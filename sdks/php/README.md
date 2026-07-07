# Adriane PHP SDK

PHP FFI wrapper over `adriane-c-api`.

```php
<?php
require __DIR__ . "/src/Adriane.php";

$adriane = Adriane::load(getenv("ADRIANE_C_API_LIB"));
echo $adriane->engineVersion() . PHP_EOL;
echo $adriane->listComponentsJson() . PHP_EOL;
```

PHP must have the FFI extension enabled.
