import hou

if hou.isUIAvailable():
    import hdefereval
    import flint_houdini

    hdefereval.executeDeferred(flint_houdini.initialize)
