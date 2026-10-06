using System;
using System.IO;
using System.Reflection;
using System.Text;

namespace Flint.Bridge
{
    public static class ManagedExecution
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

        public static string WriteCsharpMethod(string directory, string className, string code, string imports)
        {
            Directory.CreateDirectory(directory);
            var source = Path.Combine(directory, className + ".cs");
            File.WriteAllText(source, imports + "\npublic static class " + className +
                " { public static void Run() {\n" + code + "\n} }\n", Encoding.UTF8);
            return source;
        }
    }
}
