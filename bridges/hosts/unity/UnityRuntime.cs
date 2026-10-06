using System;
using Flint.Bridge;

namespace Flint.Unity
{
    // All Unity API discovery stays in the host adapter. Both delivery paths
    // load this assembly into the Editor's existing scripting domain.
    internal static class UnityRuntime
    {
        internal static readonly Type Editor = ReflectionApi.Find("UnityEditor.EditorApplication");
        internal static readonly Type Application = ReflectionApi.Find("UnityEngine.Application");
        internal static readonly Type Builder = ReflectionApi.Find("UnityEditor.Compilation.AssemblyBuilder");
        internal static readonly CallbackQueue Callbacks = new CallbackQueue();

        internal static string Version()
        {
            return "Unity " + ReflectionApi.Get(Application, "unityVersion") +
                (Type.GetType("Mono.Runtime") != null ? " Mono" : " .NET");
        }
    }
}
