python.ExecuteFile ((getFilenamePath (getSourceFileName())) + "../Python/flint_startup.py")
if menuMan.registerMenuContext 0x6f1a8d42 do
(
    local flintMenu = menuMan.createMenu "Flint"
    flintMenu.addItem (menuMan.createActionItem "FlintBridgeSettings" "Flint") -1
    local mainMenu = menuMan.getMainMenuBar()
    mainMenu.addItem (menuMan.createSubMenuItem "Flint" flintMenu) -1
    menuMan.updateMenuBar()
)
