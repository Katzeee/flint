using System;
using System.Linq;
using System.Linq.Expressions;
using System.Reflection;

namespace Flint.Bridge
{
    public static class ReflectionApi
    {
        public static Type Find(string name)
        {
            var type = AppDomain.CurrentDomain.GetAssemblies().Select(assembly => assembly.GetType(name)).FirstOrDefault(found => found != null);
            return type ?? throw new InvalidOperationException("Host API is unavailable: " + name);
        }

        public static object Get(Type type, string name)
        {
            return type.GetProperty(name, BindingFlags.Public | BindingFlags.Static).GetValue(null, null);
        }

        public static object Member(object value, string name)
        {
            var type = value.GetType();
            var field = type.GetField(name);
            return field != null ? field.GetValue(value) : type.GetProperty(name).GetValue(value, null);
        }

        public static Action Subscribe(Type type, object target, string name, Action<object[]> callback)
        {
            var eventInfo = type.GetEvent(name);
            if (eventInfo != null)
            {
                var handler = Adapt(eventInfo.EventHandlerType, callback);
                eventInfo.AddEventHandler(target, handler);
                return () => eventInfo.RemoveEventHandler(target, handler);
            }
            var field = type.GetField(name);
            if (field == null) throw new MissingMemberException(type.FullName, name);
            var entry = Adapt(field.FieldType, callback);
            field.SetValue(target, Delegate.Combine((Delegate)field.GetValue(target), entry));
            return () => field.SetValue(target, Delegate.Remove((Delegate)field.GetValue(target), entry));
        }

        /// <summary>Bind a runtime-discovered event signature without referencing its host assembly.</summary>
        private static Delegate Adapt(Type signature, Action<object[]> callback)
        {
            var parameters = signature.GetMethod("Invoke").GetParameters()
                .Select(parameter => Expression.Parameter(parameter.ParameterType)).ToArray();
            var arguments = Expression.NewArrayInit(typeof(object), parameters.Select(parameter => Expression.Convert(parameter, typeof(object))));
            return Expression.Lambda(signature, Expression.Invoke(Expression.Constant(callback), arguments), parameters).Compile();
        }

    }
}
