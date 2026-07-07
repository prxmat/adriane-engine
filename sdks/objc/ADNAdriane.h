#import <Foundation/Foundation.h>

NS_ASSUME_NONNULL_BEGIN

@interface ADNAdriane : NSObject

+ (NSString *)engineVersion;
+ (nullable NSString *)validateGraphJSON:(NSString *)definitionJSON error:(NSError **)error;
+ (nullable NSString *)compileGraphYAMLJSON:(NSString *)yaml error:(NSError **)error;
+ (nullable NSString *)availableProvidersJSON:(NSError **)error;
+ (nullable NSString *)resolveModelJSONWithTier:(NSString *)tier
                                  availableJSON:(nullable NSString *)availableJSON
                                   overrideJSON:(nullable NSString *)overrideJSON
                                          error:(NSError **)error;
+ (nullable NSString *)listComponentsJSON:(NSError **)error;
+ (nullable NSString *)listPrebuiltJSON:(NSError **)error;
+ (nullable NSString *)runComponentJSONWithKind:(NSString *)kind
                                     paramsJSON:(NSString *)paramsJSON
                                   channelsJSON:(NSString *)channelsJSON
                                          error:(NSError **)error;
+ (nullable NSString *)runPrebuiltJSONWithName:(NSString *)name
                                     inputJSON:(NSString *)inputJSON
                                   optionsJSON:(nullable NSString *)optionsJSON
                                         error:(NSError **)error;

@end

NS_ASSUME_NONNULL_END
