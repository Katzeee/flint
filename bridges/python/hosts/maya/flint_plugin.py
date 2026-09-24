"""Maya plug-in entry point for Flint Bridge."""


def initializePlugin(plugin):
    import flint_maya

    flint_maya.initialize()


def uninitializePlugin(plugin):
    import flint_maya

    flint_maya.uninitialize()
