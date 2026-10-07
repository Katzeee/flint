using System.IO;
using System.Text;

namespace Flint.Bridge
{
    public static class CsharpSource
    {
        public static string WriteMethod(string directory, string className, string code, string imports)
        {
            Directory.CreateDirectory(directory);
            var source = Path.Combine(directory, className + ".cs");
            File.WriteAllText(source, imports + "\npublic static class " + className +
                " { public static void Run() {\n" + code + "\n} }\n", Encoding.UTF8);
            return source;
        }
    }
}
