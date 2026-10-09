using System.IO;
using System.Reflection;

namespace Flint.Bridge
{
    public static class ManagedAssembly
    {
        public static void Invoke(string assemblyPath, string className)
        {
            try
            {
                var assembly = Assembly.Load(File.ReadAllBytes(assemblyPath));
                assembly.GetType(className, true).GetMethod("Run").Invoke(null, null);
            }
            catch (TargetInvocationException error) when (error.InnerException != null)
            {
                System.Runtime.ExceptionServices.ExceptionDispatchInfo.Capture(error.InnerException).Throw();
                throw;
            }
        }
    }
}
