using System;
using System.Globalization;

namespace Flint.Unity
{
    /// <summary>Decode the bootstrap request and enter the ordinary Unity adapter.</summary>
    public static class Attach
    {
        public static string Initialize(string configuration)
        {
            try
            {
                var parts = (configuration ?? string.Empty).Split('\n');
                if (parts.Length != 4) throw new ArgumentException("Incomplete attach configuration");
                EditorBridge.Attach(parts[3], parts[0], int.Parse(parts[1], CultureInfo.InvariantCulture), parts[2]);
                return null;
            }
            catch (Exception error) { return error.ToString(); }
        }
    }
}
