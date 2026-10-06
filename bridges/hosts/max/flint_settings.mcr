macroScript FlintBridgeSettings category:"Flint" toolTip:"Flint Connection Settings" buttonText:"Flint Bridge"
(
    on execute do python.Execute "import flint_max; flint_max.show_settings()"
)
