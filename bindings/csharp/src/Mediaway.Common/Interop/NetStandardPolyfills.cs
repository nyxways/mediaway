#if !NET8_0_OR_GREATER
// Marker types the C# 9+ compiler looks up by full name to allow `record`/`init`
// (IsExternalInit) and `required` members (RequiredMemberAttribute +
// CompilerFeatureRequiredAttribute). None carry runtime behavior — they ship in the
// net5.0+/net7.0+ BCL but not netstandard2.0, so each assembly building for
// netstandard2.0 links this file in via <Compile Include> to declare its own copy
// (see docs/adr/0018-csharp-netstandard20-unity.md).

namespace System.Runtime.CompilerServices
{
    internal static class IsExternalInit
    {
    }

    [AttributeUsage(AttributeTargets.Class | AttributeTargets.Struct | AttributeTargets.Field | AttributeTargets.Property, AllowMultiple = false, Inherited = false)]
    internal sealed class RequiredMemberAttribute : Attribute
    {
    }

    [AttributeUsage(AttributeTargets.Class | AttributeTargets.Struct | AttributeTargets.Field | AttributeTargets.Property | AttributeTargets.Method, AllowMultiple = true, Inherited = false)]
    internal sealed class CompilerFeatureRequiredAttribute : Attribute
    {
        public CompilerFeatureRequiredAttribute(string featureName)
        {
            FeatureName = featureName;
        }

        public string FeatureName { get; }
    }
}

namespace System.Diagnostics.CodeAnalysis
{
    [AttributeUsage(AttributeTargets.Constructor, AllowMultiple = false, Inherited = false)]
    internal sealed class SetsRequiredMembersAttribute : Attribute
    {
    }
}
#endif
